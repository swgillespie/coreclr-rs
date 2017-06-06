extern crate coreclr;
extern crate libc;

use std::ffi::CString;

fn main() {
    let ret = main_impl();
    std::process::exit(ret as i32);
}

fn main_impl() -> usize {
    let result = coreclr::ClrHostBuilder::new()
        .with_coreclr_path("/Users/sean/Documents/workspace/clr/coreclr/bin/tests/Windows_NT.x64.Release/Tests/coreoverlay")
        .with_assembly("/Users/sean/Documents/workspace/clr/misc/hello_world/bin/Debug/netcoreapp1.0/hello_world.dll")
        .with_assembly_probe_path("/Users/sean/Documents/workspace/clr/coreclr/bin/tests/Windows_NT.x64.Release/Tests/coreoverlay")
        .with_native_library_probe_path("/Users/sean/Documents/workspace/rust/coreclr/target/debug")
        .build();
    let host = match result {
        Ok(h) => h,
        Err(e) => {
            println!("error initializing coreclr: {}", e);
            return 128;
        }
    };

    let res = host.execute_assembly(&[], "/Users/sean/Documents/workspace/clr/misc/hello_world/bin/Debug/netcoreapp1.0/hello_world.dll");
    let exit_code = match res {
        Ok(exit_code) => exit_code,
        Err(exn) => {
            println!("error: {}", exn);
            128
        }
    };

    // if we don't do this, rustc doesn't emit this symbol D:
    rust_pinvoke_target(std::ptr::null_mut());

    exit_code
}

#[no_mangle]
pub extern "C" fn rust_pinvoke_target(string: *mut libc::c_char) {
    if string.is_null() {
        return;
    }

    let s = unsafe { CString::from_raw(string) };
    let string = match s.into_string() {
        Ok(s) => s,
        Err(_) => "non-UTF8 string".to_string()
    };
    println!("going back to rust with this string from C#: {}", string);
}