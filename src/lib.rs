extern crate libc;

mod loader;

use std::default::Default;
use std::path::{Path, PathBuf};
use std::io::{self, Error, ErrorKind};
use std::ffi::CString;
use std::collections::HashSet;
use std::panic;
use std::mem;
use std::fs;

struct ClrFunctions {
    initialize: extern "C" fn(    /* coreclr_initialize */
        *const libc::c_char,      /* exePath */
        *const libc::c_char,      /* appDomainFriendlyName */
        libc::c_int,              /* propertyCount */
        *mut *const libc::c_char, /* propertyKeys */
        *mut *const libc::c_char, /* propertyValues */
        *mut *mut libc::c_void,   /* hostHandle */
        *mut libc::c_uint         /* domainId */
    ) -> libc::c_int,

    shutdown: extern "C" fn(      /* coreclr_shutdown */
        *mut libc::c_void,        /* hostHandle */
        libc::c_uint              /* domainId */
    ) -> libc::c_int,

    create_delegate: extern "C" fn( /* coreclr_create_delegate */
        *mut libc::c_void,          /* hostHandle */
        libc::c_int,                /* domainId */
        *const libc::c_char,        /* entryPointAssemblyName */
        *const libc::c_char,        /* entryPointTypeName */
        *const libc::c_char,        /* entryPointMethodName */
        *mut *mut libc::c_void      /* delegate */
    ) -> libc::c_int,

    execute_assembly: extern "C" fn( /* coreclr_execute_assembly */
        *mut libc::c_void,           /* hostHandle */
        libc::c_uint,                /* domainId */
        libc::c_int,                 /* argc */
        *mut *const libc::c_char,    /* argv */
        *const libc::c_char,         /* managedAssemblyPath */
        *mut libc::c_uint            /* exitCode */
    ) -> libc::c_int
}

pub struct ClrHost {
    // this field is kept around so it can be dropped
    // at the end of ClrHost's lifetime
    #[allow(dead_code)] 
    coreclr: loader::DynamicLibrary,
    coreclr_funs: ClrFunctions,
    coreclr_handle: *mut u8,
    domain_id: usize
}

impl Drop for ClrHost {
    fn drop(&mut self) {
        (self.coreclr_funs.shutdown)(self.coreclr_handle as *mut libc::c_void, self.domain_id as libc::c_uint);
    }
}

impl ClrHost {
    pub unsafe fn create_delegate(&mut self, 
        assembly_name: &str, 
        entry_point_type_name: &str, 
        entry_point_method: &str) -> io::Result<*mut u8> {
        let mut delegate : *mut libc::c_void = std::ptr::null_mut();
        let assembly = try!(CString::new(assembly_name));
        let ty = try!(CString::new(entry_point_type_name));
        let method = try!(CString::new(entry_point_method));
        let result = (self.coreclr_funs.create_delegate)(self.coreclr_handle as *mut _, 
            self.domain_id as libc::c_int, 
            assembly.as_ptr(), 
            ty.as_ptr(), 
            method.as_ptr(),
            &mut delegate as *mut _);
        
        if result != 0 {
            Err(Error::from_raw_os_error(result))
        } else {
            Ok(delegate as *mut _)
        }
    }

    pub fn execute_assembly<T: Into<PathBuf>>(&self, args: &[&str], assembly_path: T) -> io::Result<usize> {
        let buf = assembly_path.into();
        let result = panic::catch_unwind(|| {
            self.execute_assembly_impl(args, &buf)
        });

        match result {
            Ok(Ok(return_code)) => Ok(return_code),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(Error::new(ErrorKind::Other, "unhandled CLR exception"))
        }
    }

    fn execute_assembly_impl(&self, args: &[&str], assembly_path: &Path) -> io::Result<usize> {
        let mut argv = vec![];
        for arg in args {
            argv.push(try!(CString::new(*arg)));
        }

        let asm_path = if let Some(p) = assembly_path.to_str() {
            try!(CString::new(p))
        } else {
            return Err(Error::new(ErrorKind::InvalidInput, "assembly path is not UTF-8"));
        };

        let mut pointer_vec : Vec<_> = argv.iter().map(|x| x.as_ptr()).collect();

        let mut return_code : libc::c_uint = 0;
        let result = (self.coreclr_funs.execute_assembly)(self.coreclr_handle as *mut _,
                self.domain_id as libc::c_uint,
                pointer_vec.len() as libc::c_int,
                pointer_vec.as_mut_ptr(),
                asm_path.as_ptr(),
                &mut return_code as *mut _);

        if result != 0 {
            return Err(Error::from_raw_os_error(result));
        } else {
            return Ok(return_code as usize)
        };
    }
}

pub struct ClrHostBuilder {
    server_gc: bool,
    concurrent_gc: bool,
    coreclr_path: Option<PathBuf>,
    assembly: Option<PathBuf>,
    appdomain_name: Option<String>,
    assembly_load_paths: Vec<PathBuf>,
    native_library_search_paths: Vec<PathBuf>,
}

impl Default for ClrHostBuilder {
    fn default() -> ClrHostBuilder {
        ClrHostBuilder {
            server_gc: false,
            concurrent_gc: true,
            coreclr_path: None,
            assembly: None,
            appdomain_name: None,
            assembly_load_paths: vec![],
            native_library_search_paths: vec![],
        }
    }
}

impl ClrHostBuilder {
    pub fn new() -> ClrHostBuilder {
        Default::default()
    }

    pub fn with_server_gc(&mut self) -> &mut ClrHostBuilder {
        self.server_gc = true;
        self
    }

    pub fn with_workstation_gc(&mut self) -> &mut ClrHostBuilder {
        self.server_gc = false;
        self
    }

    pub fn with_concurrent_gc(&mut self) -> &mut ClrHostBuilder {
        self.concurrent_gc = true;
        self
    }

    pub fn with_nonconcurrent_gc(&mut self) -> &mut ClrHostBuilder {
        self.concurrent_gc = false;
        self
    }

    pub fn with_coreclr_path<T: Into<PathBuf>>(&mut self, path: T) -> &mut ClrHostBuilder {
        self.coreclr_path = Some(path.into());
        self
    }

    pub fn with_assembly_probe_path<T: Into<PathBuf>>(&mut self, path: T) -> &mut ClrHostBuilder {
        self.assembly_load_paths.push(path.into());
        self
    }

    pub fn with_native_library_probe_path<T: Into<PathBuf>>(&mut self, path: T) -> &mut ClrHostBuilder {
        self.native_library_search_paths.push(path.into());
        self
    }

    pub fn with_assembly<T: Into<PathBuf>>(&mut self, path: T) -> &mut ClrHostBuilder {
        self.assembly = Some(path.into());
        self
    }

    pub fn with_appdomain_name<T: Into<String>>(&mut self, name: T) -> &mut ClrHostBuilder {
        self.appdomain_name = Some(name.into());
        self
    }

    pub fn build(&self) -> io::Result<ClrHost> {
        let coreclr_path = if let Some(ref p) = self.coreclr_path {
            if let Some(s) = p.to_str() {
                s.to_string()
            } else {
                return Err(Error::new(ErrorKind::InvalidInput, "coreclr path is not valid UTF-8"));
            }
        } else {
            return Err(Error::new(ErrorKind::NotFound, "no path to coreclr provided"));
        };

        let assembly_path = if let Some(ref p) = self.assembly {
            if let Some(s) = p.to_str() {
                s.to_string()
            } else {
                return Err(Error::new(ErrorKind::InvalidInput, "assembly path is not valid UTF-8"));
            }
        } else {
            return Err(Error::new(ErrorKind::NotFound, "no assembly provided"));
        }; 

        // every list expected by the runtime here is colon-delimited.

        // first - building native search directory paths.
        // by default, the directory where libcoreclr resides is
        // probed by the runtime for PInvoke targets.
        let mut native_search_path = String::new();
        native_search_path.push_str(&coreclr_path);
        for path in self.native_library_search_paths.iter() {
            if let Some(s) = path.to_str() {
                native_search_path.push(':');
                native_search_path.push_str(s);
            } else {
                return Err(Error::new(ErrorKind::InvalidInput, "native search path is not valid UTF-8"));
            }
        }

        // second - load coreclr.
        let actual_path = self.coreclr_path.clone().unwrap();
        let mut coreclr_path = actual_path.clone();
        if cfg!(target_os = "macos") {
            coreclr_path.push("libcoreclr.dylib");
        } else if cfg!(target_os = "linux") {
            coreclr_path.push("libcoreclr.so");
        } else {
            coreclr_path.push("coreclr.dll");
        }

        let lib = try!(loader::DynamicLibrary::load(&coreclr_path));
        // load our function pointers.
        let functions = unsafe {
            ClrFunctions {
                initialize: mem::transmute(try!(lib.resolve_symbol("coreclr_initialize"))),
                shutdown: mem::transmute(try!(lib.resolve_symbol("coreclr_shutdown"))),
                create_delegate: mem::transmute(try!(lib.resolve_symbol("coreclr_create_delegate"))),
                execute_assembly: mem::transmute(try!(lib.resolve_symbol("coreclr_execute_assembly")))
            }
        };

        // build up CStrings to send to coreclr.
        let mut probe_paths = String::new();
        for path in &self.assembly_load_paths {
            if let Some(s) = path.to_str() {
                if probe_paths.len() != 0 {
                    probe_paths.push(':');
                }

                probe_paths.push_str(s);
            } else {
                return Err(Error::new(ErrorKind::InvalidInput, "native search path is not valid UTF-8"));
            }
        }

        let tpa = try!(build_tpas(&actual_path.clone()));

        let assembly = CString::new(assembly_path).unwrap();
        let name = if let Some(ref s) = self.appdomain_name {
            try!(CString::new(s.clone()))
        } else {
            CString::new("rust_coreclr_host").unwrap()
        };

        let server_gc = if self.server_gc {
            CString::new("true").unwrap()
        } else {
            CString::new("false").unwrap()
        };

        let concurrent_gc = if self.concurrent_gc {
            CString::new("true").unwrap()
        } else {
            CString::new("false").unwrap()
        };

        let property_keys = vec![
            CString::new("TRUSTED_PLATFORM_ASSEMBLIES").unwrap(),
            CString::new("APP_PATHS").unwrap(),
            CString::new("APP_NI_PATHS").unwrap(),
            CString::new("NATIVE_DLL_SEARCH_DIRECTORIES").unwrap(),
            CString::new("AppDomainCompatSwitch").unwrap(),
            CString::new("System.GC.Server").unwrap(),
            CString::new("System.GC.Concurrent").unwrap()
        ];

        let property_values = vec![
            CString::new(tpa).unwrap(),
            CString::new(probe_paths.clone()).unwrap(),
            CString::new(probe_paths).unwrap(),
            CString::new(native_search_path).unwrap(),
            CString::new("UseLatestBehaviorWhenTFMNotSpecified").unwrap(),
            server_gc,
            concurrent_gc
        ];

        assert!(property_keys.len() == property_values.len());

        // initialize!
        let mut property_keys_raw : Vec<_> = property_keys.iter().map(|p| p.as_ptr()).collect();
        let mut property_values_raw : Vec<_> = property_values.iter().map(|p| p.as_ptr()).collect();

        let mut handle : *mut libc::c_void = std::ptr::null_mut();
        let mut domain_id : libc::c_uint = 0;
        let result = (functions.initialize)(
            assembly.as_ptr(),
            name.as_ptr(),
            property_keys.len() as libc::c_int,
            property_keys_raw.as_mut_ptr(),
            property_values_raw.as_mut_ptr(),
            &mut handle as *mut _,
            &mut domain_id as *mut _
        );

        if result != 0 {
            Err(Error::from_raw_os_error(result))
        } else {
            Ok(ClrHost {
                coreclr: lib,
                coreclr_funs: functions,
                coreclr_handle: handle as *mut _,
                domain_id: domain_id as usize
            })
        }
    }
}

fn build_tpas(path: &Path) -> io::Result<String> {
    let mut buffer = vec![];
    let mut set = HashSet::new();
    for file in try!(fs::read_dir(path)) {
        let actual_file = try!(file);
        if let Some(p) = actual_file.path().extension() {
            if let Some(s) = p.to_str() {
                match s {
                    "dll" | "exe" => {
                        let file = actual_file.path().clone();

                        // don't want to insert duplicates
                        if set.insert(file.clone()) {
                            buffer.push(actual_file
                                .path()
                                .to_str()
                                .unwrap()
                                .to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(buffer.join(":"))
}