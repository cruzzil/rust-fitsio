//! Backend calling the C CFITSIO library through `fitsio-sys`
//!
//! The safety argument for every call is the same: the `fitsfile` pointer is owned by a live
//! [`Handle`], every buffer is a Rust slice at least as long as CFITSIO is told it is, and every
//! string is nul-terminated.

// C uses long argument lists
#![allow(clippy::too_many_arguments)]

use super::{c_char, c_int, c_long, FLEN_COMMENT, FLEN_FILENAME, FLEN_STATUS, FLEN_VALUE};
use crate::types::HasFitsDataType;
use fitsio_sys as sys;
use std::ffi::c_void;
use std::ffi::{CStr, CString};
use std::ptr::{self, NonNull};

/// An open FITS file, closed when dropped
pub(crate) struct Handle(NonNull<sys::fitsfile>);

impl Handle {
    pub(crate) fn open(path: &CStr, mode: c_int, status: &mut c_int) -> Option<Self> {
        let mut fptr = ptr::null_mut();
        unsafe {
            sys::ffopen(&mut fptr, path.as_ptr(), mode, status);
        }
        NonNull::new(fptr).map(Handle)
    }

    pub(crate) fn create(path: &CStr, status: &mut c_int) -> Option<Self> {
        let mut fptr = ptr::null_mut();
        unsafe {
            sys::ffinit(&mut fptr, path.as_ptr(), status);
        }
        NonNull::new(fptr).map(Handle)
    }

    /// Take ownership of a `fitsfile` opened by the caller
    ///
    /// # Safety
    ///
    /// `fptr` must point to an open `fitsfile`, created by CFITSIO, that nothing else will
    /// close.
    pub(crate) unsafe fn from_raw(fptr: NonNull<sys::fitsfile>) -> Self {
        Handle(fptr)
    }

    pub(crate) fn as_raw(&mut self) -> *mut sys::fitsfile {
        self.0.as_ptr()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let mut status = 0;
        unsafe {
            sys::ffclos(self.0.as_ptr(), &mut status);
        }
    }
}

/// Values that can be written to a header keyword
pub(crate) trait KeyValue: HasFitsDataType {}

/// Values that can also be read from a header keyword
pub(crate) trait ReadKeyValue: KeyValue {}

macro_rules! key_value_impl {
    ($tr:ident: $($t:ty),*) => {
        $(impl $tr for $t {})*
    };
}

key_value_impl!(KeyValue: i8, i16, i32, i64, u8, u16, u32, u64, f32, f64);
key_value_impl!(ReadKeyValue: i32, i64, f32, f64);

fn opt_ptr(s: Option<&CStr>) -> *const c_char {
    s.map_or(ptr::null(), CStr::as_ptr)
}

pub(crate) fn fits_file_mode(f: &mut Handle, mode: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffflmd(f.as_raw(), mode, status);
    }
}

pub(crate) fn fits_file_name(
    f: &mut Handle,
    name: &mut [c_char; FLEN_FILENAME],
    status: &mut c_int,
) {
    unsafe {
        sys::ffflnm(f.as_raw(), name.as_mut_ptr(), status);
    }
}

pub(crate) fn fits_write_imghdr(
    f: &mut Handle,
    bitpix: c_int,
    naxes: &[c_long],
    status: &mut c_int,
) {
    unsafe {
        sys::ffphps(
            f.as_raw(),
            bitpix,
            naxes.len() as c_int,
            naxes.as_ptr() as *mut c_long,
            status,
        );
    }
}

pub(crate) fn fits_get_num_hdus(f: &mut Handle, num: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffthdu(f.as_raw(), num, status);
    }
}

pub(crate) fn fits_get_hdu_num(f: &mut Handle) -> c_int {
    let mut num = 0;
    unsafe {
        sys::ffghdn(f.as_raw(), &mut num);
    }
    num
}

pub(crate) fn fits_get_hdu_type(f: &mut Handle, hdutype: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffghdt(f.as_raw(), hdutype, status);
    }
}

pub(crate) fn fits_get_img_dim(f: &mut Handle, naxis: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffgidm(f.as_raw(), naxis, status);
    }
}

pub(crate) fn fits_get_img_size(f: &mut Handle, naxes: &mut [c_long], status: &mut c_int) {
    unsafe {
        sys::ffgisz(f.as_raw(), naxes.len() as c_int, naxes.as_mut_ptr(), status);
    }
}

pub(crate) fn fits_get_img_equivtype(f: &mut Handle, bitpix: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffgiet(f.as_raw(), bitpix, status);
    }
}

pub(crate) fn fits_get_num_rows(f: &mut Handle, nrows: &mut c_long, status: &mut c_int) {
    unsafe {
        sys::ffgnrw(f.as_raw(), nrows, status);
    }
}

pub(crate) fn fits_get_num_cols(f: &mut Handle, ncols: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffgncl(f.as_raw(), ncols, status);
    }
}

/// Fetch a binary table column's name, datatype code (e.g. `1J`, `PE`) and repeat count
pub(crate) fn fits_get_bcolparms(
    f: &mut Handle,
    colnum: c_int,
    ttype: &mut [c_char; FLEN_VALUE],
    dtype: &mut [c_char; FLEN_VALUE],
    repeat: &mut c_long,
    status: &mut c_int,
) {
    unsafe {
        sys::ffgbcl(
            f.as_raw(),
            colnum,
            ttype.as_mut_ptr(),
            ptr::null_mut(),
            dtype.as_mut_ptr(),
            repeat,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            status,
        );
    }
}

pub(crate) fn fits_create_tbl(
    f: &mut Handle,
    tbltype: c_int,
    naxis2: i64,
    ttype: &[CString],
    tform: &[CString],
    extname: Option<&CStr>,
    status: &mut c_int,
) {
    assert_eq!(ttype.len(), tform.len());
    // CFITSIO does not write through these, despite taking `char **`
    let mut ttype: Vec<*mut c_char> = ttype.iter().map(|s| s.as_ptr() as *mut c_char).collect();
    let mut tform: Vec<*mut c_char> = tform.iter().map(|s| s.as_ptr() as *mut c_char).collect();
    unsafe {
        sys::ffcrtb(
            f.as_raw(),
            tbltype,
            naxis2,
            ttype.len() as c_int,
            ttype.as_mut_ptr(),
            tform.as_mut_ptr(),
            ptr::null_mut(),
            opt_ptr(extname),
            status,
        );
    }
}

pub(crate) fn fits_create_img(f: &mut Handle, bitpix: c_int, naxes: &[c_long], status: &mut c_int) {
    unsafe {
        sys::ffcrim(
            f.as_raw(),
            bitpix,
            naxes.len() as c_int,
            naxes.as_ptr() as *mut c_long,
            status,
        );
    }
}

pub(crate) fn fits_resize_img(f: &mut Handle, bitpix: c_int, naxes: &[c_long], status: &mut c_int) {
    unsafe {
        sys::ffrsim(
            f.as_raw(),
            bitpix,
            naxes.len() as c_int,
            naxes.as_ptr() as *mut c_long,
            status,
        );
    }
}

pub(crate) fn fits_copy_hdu(
    src: &mut Handle,
    dest: &mut Handle,
    morekeys: c_int,
    status: &mut c_int,
) {
    unsafe {
        sys::ffcopy(src.as_raw(), dest.as_raw(), morekeys, status);
    }
}

pub(crate) fn fits_insert_col(
    f: &mut Handle,
    colnum: c_int,
    ttype: &CStr,
    tform: &CStr,
    status: &mut c_int,
) {
    unsafe {
        sys::fficol(
            f.as_raw(),
            colnum,
            ttype.as_ptr() as *mut c_char,
            tform.as_ptr() as *mut c_char,
            status,
        );
    }
}

pub(crate) fn fits_delete_col(f: &mut Handle, colnum: c_int, status: &mut c_int) {
    unsafe {
        sys::ffdcol(f.as_raw(), colnum, status);
    }
}

pub(crate) fn fits_get_colnum(
    f: &mut Handle,
    casesen: c_int,
    templt: &CStr,
    colnum: &mut c_int,
    status: &mut c_int,
) {
    unsafe {
        sys::ffgcno(
            f.as_raw(),
            casesen,
            templt.as_ptr() as *mut c_char,
            colnum,
            status,
        );
    }
}

pub(crate) fn fits_delete_hdu(f: &mut Handle, hdutype: &mut c_int, status: &mut c_int) {
    unsafe {
        sys::ffdhdu(f.as_raw(), hdutype, status);
    }
}

pub(crate) fn fits_movabs_hdu(
    f: &mut Handle,
    hdunum: c_int,
    hdutype: &mut c_int,
    status: &mut c_int,
) {
    unsafe {
        sys::ffmahd(f.as_raw(), hdunum, hdutype, status);
    }
}

pub(crate) fn fits_movnam_hdu(
    f: &mut Handle,
    hdutype: c_int,
    extname: &CStr,
    extver: c_int,
    status: &mut c_int,
) {
    unsafe {
        sys::ffmnhd(
            f.as_raw(),
            hdutype,
            extname.as_ptr() as *mut c_char,
            extver,
            status,
        );
    }
}

pub(crate) fn fits_read_key<T: ReadKeyValue>(
    f: &mut Handle,
    keyname: &CStr,
    value: &mut T,
    comment: &mut [c_char; FLEN_COMMENT],
    status: &mut c_int,
) {
    unsafe {
        sys::ffgky(
            f.as_raw(),
            T::FITS_DATA_TYPE.into(),
            keyname.as_ptr(),
            value as *mut T as *mut c_void,
            comment.as_mut_ptr(),
            status,
        );
    }
}

pub(crate) fn fits_read_key_str(
    f: &mut Handle,
    keyname: &CStr,
    value: &mut [c_char; FLEN_VALUE],
    comment: &mut [c_char; FLEN_COMMENT],
    status: &mut c_int,
) {
    unsafe {
        sys::ffgkys(
            f.as_raw(),
            keyname.as_ptr(),
            value.as_mut_ptr(),
            comment.as_mut_ptr(),
            status,
        );
    }
}

pub(crate) fn fits_write_key<T: KeyValue>(
    f: &mut Handle,
    keyname: &CStr,
    value: &T,
    comment: Option<&CStr>,
    status: &mut c_int,
) {
    unsafe {
        sys::ffpky(
            f.as_raw(),
            T::FITS_DATA_TYPE.into(),
            keyname.as_ptr(),
            value as *const T as *mut c_void,
            opt_ptr(comment),
            status,
        );
    }
}

pub(crate) fn fits_write_key_str(
    f: &mut Handle,
    keyname: &CStr,
    value: &CStr,
    comment: Option<&CStr>,
    status: &mut c_int,
) {
    unsafe {
        sys::ffpkys(
            f.as_raw(),
            keyname.as_ptr(),
            value.as_ptr(),
            opt_ptr(comment),
            status,
        );
    }
}

/// Read `nelem` pixels of type `datatype` into `array`, which must hold at least that many
pub(crate) fn fits_read_img(
    f: &mut Handle,
    datatype: c_int,
    firstelem: i64,
    nelem: i64,
    array: &mut [u8],
    status: &mut c_int,
) {
    unsafe {
        sys::ffgpv(
            f.as_raw(),
            datatype,
            firstelem,
            nelem,
            ptr::null_mut(),
            array.as_mut_ptr() as *mut c_void,
            ptr::null_mut(),
            status,
        );
    }
}

pub(crate) fn fits_read_subset(
    f: &mut Handle,
    datatype: c_int,
    fpixel: &[c_long],
    lpixel: &[c_long],
    inc: &[c_long],
    array: &mut [u8],
    status: &mut c_int,
) {
    unsafe {
        sys::ffgsv(
            f.as_raw(),
            datatype,
            fpixel.as_ptr() as *mut c_long,
            lpixel.as_ptr() as *mut c_long,
            inc.as_ptr() as *mut c_long,
            ptr::null_mut(),
            array.as_mut_ptr() as *mut c_void,
            ptr::null_mut(),
            status,
        );
    }
}

pub(crate) fn fits_write_img(
    f: &mut Handle,
    datatype: c_int,
    firstelem: i64,
    nelem: i64,
    array: &[u8],
    status: &mut c_int,
) {
    unsafe {
        sys::ffppr(
            f.as_raw(),
            datatype,
            firstelem,
            nelem,
            array.as_ptr() as *mut c_void,
            status,
        );
    }
}

pub(crate) fn fits_write_subset(
    f: &mut Handle,
    datatype: c_int,
    fpixel: &[c_long],
    lpixel: &[c_long],
    array: &[u8],
    status: &mut c_int,
) {
    unsafe {
        sys::ffpss(
            f.as_raw(),
            datatype,
            fpixel.as_ptr() as *mut c_long,
            lpixel.as_ptr() as *mut c_long,
            array.as_ptr() as *mut c_void,
            status,
        );
    }
}

/// Read `nelem` values of type `datatype` from a column, without checking for nulls
pub(crate) fn fits_read_col(
    f: &mut Handle,
    datatype: c_int,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    nelem: i64,
    array: &mut [u8],
    status: &mut c_int,
) {
    unsafe {
        sys::ffgcv(
            f.as_raw(),
            datatype,
            colnum,
            firstrow,
            firstelem,
            nelem,
            ptr::null_mut(),
            array.as_mut_ptr() as *mut c_void,
            ptr::null_mut(),
            status,
        );
    }
}

pub(crate) fn fits_read_col_log(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    nulval: c_char,
    array: &mut [c_char],
    status: &mut c_int,
) {
    unsafe {
        sys::ffgcvl(
            f.as_raw(),
            colnum,
            firstrow,
            firstelem,
            array.len() as i64,
            nulval,
            array.as_mut_ptr(),
            ptr::null_mut(),
            status,
        );
    }
}

/// Read one string per element of `array`; each buffer must have room for the column's
/// display width plus a terminating nul
pub(crate) fn fits_read_col_str(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    array: &mut [Vec<c_char>],
    status: &mut c_int,
) {
    let mut ptrs: Vec<*mut c_char> = array.iter_mut().map(|s| s.as_mut_ptr()).collect();
    unsafe {
        sys::ffgcvs(
            f.as_raw(),
            colnum,
            firstrow,
            firstelem,
            ptrs.len() as i64,
            ptr::null_mut(),
            ptrs.as_mut_ptr(),
            ptr::null_mut(),
            status,
        );
    }
}

pub(crate) fn fits_write_col(
    f: &mut Handle,
    datatype: c_int,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    nelem: i64,
    array: &[u8],
    status: &mut c_int,
) {
    unsafe {
        sys::ffpcl(
            f.as_raw(),
            datatype,
            colnum,
            firstrow,
            firstelem,
            nelem,
            array.as_ptr() as *mut c_void,
            status,
        );
    }
}

pub(crate) fn fits_write_col_str(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    array: &[CString],
    status: &mut c_int,
) {
    // CFITSIO does not write through these, despite taking `char **`
    let mut ptrs: Vec<*mut c_char> = array.iter().map(|s| s.as_ptr() as *mut c_char).collect();
    unsafe {
        sys::ffpcls(
            f.as_raw(),
            colnum,
            firstrow,
            firstelem,
            ptrs.len() as i64,
            ptrs.as_mut_ptr(),
            status,
        );
    }
}

pub(crate) fn fits_get_col_display_width(
    f: &mut Handle,
    colnum: c_int,
    width: &mut c_int,
    status: &mut c_int,
) {
    unsafe {
        sys::ffgcdw(f.as_raw(), colnum, width, status);
    }
}

pub(crate) fn fits_get_errstatus(status: c_int, text: &mut [c_char; FLEN_STATUS]) {
    unsafe {
        sys::ffgerr(status, text.as_mut_ptr());
    }
}
