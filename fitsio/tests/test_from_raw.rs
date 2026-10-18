// Uses the C `fitsio-sys` API directly
#![cfg(feature = "cfitsio")]

use fitsio::{sys::ffopen, FileOpenMode, FitsFile};
use std::ptr;

#[test]
fn from_raw() {
    let filename = "../testdata/full_example.fits";
    let mut fptr = ptr::null_mut();
    let mut status = 0;
    let c_filename = std::ffi::CString::new(filename).expect("filename is not a valid C-string");

    unsafe {
        ffopen(
            &mut fptr as *mut *mut _,
            c_filename.as_ptr(),
            0, // readonly
            &mut status,
        );
    }
    assert_eq!(status, 0);

    let mut f = unsafe { FitsFile::from_raw(fptr, FileOpenMode::READONLY) }.unwrap();

    // the rest of this test is taken from the `images.rs::test_read_image_data` test.
    let hdu = f.hdu(0).unwrap();
    let first_row: Vec<i32> = hdu.read_section(&mut f, 0, 100).unwrap();
    assert_eq!(first_row.len(), 100);
    assert_eq!(first_row[0], 108);
    assert_eq!(first_row[49], 176);

    let second_row: Vec<i32> = hdu.read_section(&mut f, 100, 200).unwrap();
    assert_eq!(second_row.len(), 100);
    assert_eq!(second_row[0], 177);
    assert_eq!(second_row[49], 168);
}

/// When `from_raw` fails, the caller still owns the pointer, so it must not have been closed
// Linux accepts the non-UTF-8 file name this needs; macOS (APFS) does not
#[cfg(target_os = "linux")]
#[test]
fn from_raw_error_leaves_pointer_open() {
    use fitsio::sys::{ffclos, ffcrim, ffinit};
    use std::os::unix::ffi::OsStrExt;

    let dir = tempfile::tempdir().unwrap();
    // a file name that is not UTF-8, which `from_raw` cannot represent
    let path = dir.path().join(std::ffi::OsStr::from_bytes(b"\xff.fits"));
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();

    let mut fptr = ptr::null_mut();
    let mut status = 0;
    unsafe {
        ffinit(&mut fptr, c_path.as_ptr(), &mut status);
        ffcrim(fptr, 8, 0, ptr::null_mut(), &mut status);
    }
    assert_eq!(status, 0);

    assert!(unsafe { FitsFile::from_raw(fptr, FileOpenMode::READWRITE) }.is_err());

    unsafe {
        ffclos(fptr, &mut status);
    }
    assert_eq!(status, 0);
}
