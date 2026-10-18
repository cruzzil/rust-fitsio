// Uses the `rsfitsio` API directly
#![cfg(all(feature = "rsfitsio-experimental", not(feature = "cfitsio")))]

use fitsio::{errors::check_status, FileOpenMode, FitsFile};
use rsfitsio::aliases::rust_api::fits_open_memfile;
use rsfitsio::fitsio::READONLY;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Size of the buffer holding the file
static BUFFER_LEN: AtomicUsize = AtomicUsize::new(0);

/// The buffer is fixed, so "reallocating" only succeeds while it is large enough. rsfitsio
/// truncates read-only files to their own size when moving between HDUs.
unsafe extern "C" fn fixed_realloc(p: *mut c_void, newsize: usize) -> *mut c_void {
    if newsize <= BUFFER_LEN.load(Ordering::SeqCst) {
        p
    } else {
        std::ptr::null_mut()
    }
}

#[test]
fn reading_from_memory() {
    let mut bytes = std::fs::read("../testdata/full_example.fits").unwrap();
    let mut size = bytes.len();
    BUFFER_LEN.store(size, Ordering::SeqCst);
    let mut ptr = bytes.as_mut_ptr() as *mut c_void;

    let name: Vec<_> = b"full_example.fits\0".iter().map(|&b| b as _).collect();
    let mut fptr = None;
    let mut status = 0;
    fits_open_memfile(
        &mut fptr,
        &name,
        READONLY,
        &mut ptr,
        &mut size,
        0,
        fixed_realloc,
        &mut status,
    );
    check_status(status).unwrap();

    let mut f = FitsFile::from_rsfitsio(fptr.unwrap(), FileOpenMode::READONLY).unwrap();
    assert_eq!(f.num_hdus().unwrap(), 2);
    f.pretty_print().expect("pretty printing fits file");
}
