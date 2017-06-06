//! Dynamic library loader for Unixes.
use libc;
use std::path::Path;
use std::ffi::CString;
use std::io::{self, Error, ErrorKind};

pub struct DynamicLibrary {
    handle: *mut libc::c_void
}

impl DynamicLibrary {
    pub fn load(path: &Path) -> io::Result<DynamicLibrary> {
        let cstr = if let Some(s) = path.to_str() {
            try!(CString::new(s))
        } else {
            return Err(Error::new(ErrorKind::InvalidInput, "non-UTF8 path"));
        };

        let handle = unsafe { libc::dlopen(cstr.as_ptr(), 0x2 /* RTLD_NOW */) };
        if handle.is_null() {
            unsafe {
                let err = libc::dlerror();
                let string = CString::from_raw(err);
                let actual_string = string.into_string().expect("the OS gave us a non-UTF8 string");
                Err(Error::new(ErrorKind::Other, actual_string))
            }
        } else {
            Ok(DynamicLibrary {
                handle: handle
            })
        }
    }

    pub unsafe fn resolve_symbol<T: Into<Vec<u8>>>(&self, name: T) -> io::Result<*mut libc::c_void> {
        let cstr = try!(CString::new(name));

        let result = libc::dlsym(self.handle, cstr.as_ptr());
        if result.is_null() {
            let err = libc::dlerror();
            let string = CString::from_raw(err);
            let actual_string = string.into_string().expect("the OS gave us a non-UTF8 string");
            Err(Error::new(ErrorKind::Other, actual_string))
        } else {
            Ok(result)
        }
    }
}

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}