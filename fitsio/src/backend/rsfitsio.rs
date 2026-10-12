//! Backend calling the safe API of the pure Rust `rsfitsio` port of CFITSIO
//!
//! Unlike the C backend this needs no `unsafe`: `rsfitsio` takes references, slices and
//! `Option`s, and an open file is an owned `Box<fitsfile>`.

// C uses long argument lists
#![allow(clippy::too_many_arguments)]
#![forbid(unsafe_code)]

use super::{c_char, c_int, c_long, FLEN_COMMENT, FLEN_FILENAME, FLEN_STATUS, FLEN_VALUE};
use bytemuck::cast_slice;
use rsfitsio::aliases::rust_api as api;
use rsfitsio::fitsio::fitsfile;
use rsfitsio::{KeywordDatatype, KeywordDatatypeMut};
use std::ffi::{CStr, CString};

/// An open FITS file, closed when dropped
pub(crate) struct Handle(Option<Box<fitsfile>>);

impl Handle {
    pub(crate) fn open(path: &CStr, mode: c_int, status: &mut c_int) -> Option<Self> {
        let mut fptr = None;
        api::fits_open_file(&mut fptr, chars(path), mode, status);
        fptr.map(|f| Handle(Some(f)))
    }

    pub(crate) fn create(path: &CStr, status: &mut c_int) -> Option<Self> {
        let mut fptr = None;
        api::fits_create_file(&mut fptr, chars(path), status);
        fptr.map(|f| Handle(Some(f)))
    }

    /// Take ownership of a file opened through `rsfitsio`
    pub(crate) fn from_rsfitsio(fptr: Box<fitsfile>) -> Self {
        Handle(Some(fptr))
    }

    pub(crate) fn as_rsfitsio(&mut self) -> &mut fitsfile {
        self.0.as_mut().expect("file is only closed on drop")
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Some(fptr) = self.0.take() {
            let mut status = 0;
            api::fits_close_file(fptr, &mut status);
        }
    }
}

/// A nul-terminated string as the `c_char` slice `rsfitsio` takes
fn chars(s: &CStr) -> &[c_char] {
    cast_slice(s.to_bytes_with_nul())
}

/// Values that can be written to a header keyword
pub(crate) trait KeyValue {
    fn keyword(&self) -> KeywordDatatype<'_>;
}

/// Values that can also be read from a header keyword
pub(crate) trait ReadKeyValue: KeyValue {
    fn keyword_mut(&mut self) -> KeywordDatatypeMut<'_>;
}

macro_rules! key_value_impl {
    ($($t:ty => $variant:ident),* $(,)?) => {
        $(
            impl KeyValue for $t {
                fn keyword(&self) -> KeywordDatatype<'_> {
                    KeywordDatatype::$variant(self)
                }
            }
        )*
    };
}

macro_rules! read_key_value_impl {
    ($($t:ty => $variant:ident),* $(,)?) => {
        $(
            impl ReadKeyValue for $t {
                fn keyword_mut(&mut self) -> KeywordDatatypeMut<'_> {
                    KeywordDatatypeMut::$variant(self)
                }
            }
        )*
    };
}

key_value_impl!(
    i8 => TSBYTE,
    u8 => TBYTE,
    i16 => TSHORT,
    u16 => TUSHORT,
    i32 => TINT,
    u32 => TUINT,
    i64 => TLONGLONG,
    u64 => TULONGLONG,
    f32 => TFLOAT,
    f64 => TDOUBLE,
);

read_key_value_impl!(
    i32 => TINT,
    i64 => TLONGLONG,
    f32 => TFLOAT,
    f64 => TDOUBLE,
);

pub(crate) fn fits_file_mode(f: &mut Handle, mode: &mut c_int, status: &mut c_int) {
    api::fits_file_mode(f.as_rsfitsio(), mode, status);
}

pub(crate) fn fits_file_name(
    f: &mut Handle,
    name: &mut [c_char; FLEN_FILENAME],
    status: &mut c_int,
) {
    api::fits_file_name(f.as_rsfitsio(), name, status);
}

pub(crate) fn fits_write_imghdr(
    f: &mut Handle,
    bitpix: c_int,
    naxes: &[c_long],
    status: &mut c_int,
) {
    api::fits_write_imghdr(f.as_rsfitsio(), bitpix, naxes.len() as c_int, naxes, status);
}

pub(crate) fn fits_get_num_hdus(f: &mut Handle, num: &mut c_int, status: &mut c_int) {
    api::fits_get_num_hdus(f.as_rsfitsio(), num, status);
}

pub(crate) fn fits_get_hdu_num(f: &mut Handle) -> c_int {
    let mut num = 0;
    api::fits_get_hdu_num(f.as_rsfitsio(), &mut num);
    num
}

pub(crate) fn fits_get_hdu_type(f: &mut Handle, hdutype: &mut c_int, status: &mut c_int) {
    api::fits_get_hdu_type(f.as_rsfitsio(), hdutype, status);
}

pub(crate) fn fits_get_img_dim(f: &mut Handle, naxis: &mut c_int, status: &mut c_int) {
    api::fits_get_img_dim(f.as_rsfitsio(), naxis, status);
}

pub(crate) fn fits_get_img_size(f: &mut Handle, naxes: &mut [c_long], status: &mut c_int) {
    api::fits_get_img_size(f.as_rsfitsio(), naxes.len() as c_int, naxes, status);
}

pub(crate) fn fits_get_img_equivtype(f: &mut Handle, bitpix: &mut c_int, status: &mut c_int) {
    api::fits_get_img_equivtype(f.as_rsfitsio(), bitpix, status);
}

pub(crate) fn fits_get_num_rows(f: &mut Handle, nrows: &mut c_long, status: &mut c_int) {
    api::fits_get_num_rows(f.as_rsfitsio(), nrows, status);
}

pub(crate) fn fits_get_num_cols(f: &mut Handle, ncols: &mut c_int, status: &mut c_int) {
    api::fits_get_num_cols(f.as_rsfitsio(), ncols, status);
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
    // TODO: rsfitsio's `dtype` only has room for one character and the nul, so it cannot hold
    // the `P` prefix of a variable length column.
    let mut code: [c_char; 2] = [0; 2];
    api::fits_get_bcolparms(
        f.as_rsfitsio(),
        colnum,
        Some(ttype),
        None,
        Some(&mut code),
        Some(repeat),
        None,
        None,
        None,
        None,
        status,
    );
    dtype[..2].copy_from_slice(&code);
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
    let ttype: Vec<Option<&[c_char]>> = ttype.iter().map(|s| Some(chars(s))).collect();
    let tform: Vec<&[c_char]> = tform.iter().map(|s| chars(s)).collect();
    api::fits_create_tbl(
        f.as_rsfitsio(),
        tbltype,
        naxis2,
        ttype.len() as c_int,
        &ttype,
        &tform,
        None,
        extname.map(chars),
        status,
    );
}

pub(crate) fn fits_create_img(f: &mut Handle, bitpix: c_int, naxes: &[c_long], status: &mut c_int) {
    api::fits_create_img(f.as_rsfitsio(), bitpix, naxes.len() as c_int, naxes, status);
}

pub(crate) fn fits_resize_img(f: &mut Handle, bitpix: c_int, naxes: &[c_long], status: &mut c_int) {
    api::fits_resize_img(f.as_rsfitsio(), bitpix, naxes.len() as c_int, naxes, status);
}

pub(crate) fn fits_copy_hdu(
    src: &mut Handle,
    dest: &mut Handle,
    morekeys: c_int,
    status: &mut c_int,
) {
    api::fits_copy_hdu(src.as_rsfitsio(), dest.as_rsfitsio(), morekeys, status);
}

pub(crate) fn fits_insert_col(
    f: &mut Handle,
    colnum: c_int,
    ttype: &CStr,
    tform: &CStr,
    status: &mut c_int,
) {
    api::fits_insert_col(f.as_rsfitsio(), colnum, chars(ttype), chars(tform), status);
}

pub(crate) fn fits_delete_col(f: &mut Handle, colnum: c_int, status: &mut c_int) {
    api::fits_delete_col(f.as_rsfitsio(), colnum, status);
}

pub(crate) fn fits_get_colnum(
    f: &mut Handle,
    casesen: c_int,
    templt: &CStr,
    colnum: &mut c_int,
    status: &mut c_int,
) {
    api::fits_get_colnum(f.as_rsfitsio(), casesen, chars(templt), colnum, status);
}

pub(crate) fn fits_delete_hdu(f: &mut Handle, hdutype: &mut c_int, status: &mut c_int) {
    api::fits_delete_hdu(f.as_rsfitsio(), Some(hdutype), status);
}

pub(crate) fn fits_movabs_hdu(
    f: &mut Handle,
    hdunum: c_int,
    hdutype: &mut c_int,
    status: &mut c_int,
) {
    api::fits_movabs_hdu(f.as_rsfitsio(), hdunum, Some(hdutype), status);
}

pub(crate) fn fits_movnam_hdu(
    f: &mut Handle,
    hdutype: c_int,
    extname: &CStr,
    extver: c_int,
    status: &mut c_int,
) {
    api::fits_movnam_hdu(f.as_rsfitsio(), hdutype, chars(extname), extver, status);
}

pub(crate) fn fits_read_key<T: ReadKeyValue>(
    f: &mut Handle,
    keyname: &CStr,
    value: &mut T,
    comment: &mut [c_char; FLEN_COMMENT],
    status: &mut c_int,
) {
    api::fits_read_key(
        f.as_rsfitsio(),
        value.keyword_mut(),
        chars(keyname),
        Some(comment),
        status,
    );
}

pub(crate) fn fits_read_key_str(
    f: &mut Handle,
    keyname: &CStr,
    value: &mut [c_char; FLEN_VALUE],
    comment: &mut [c_char; FLEN_COMMENT],
    status: &mut c_int,
) {
    api::fits_read_key_str(
        f.as_rsfitsio(),
        chars(keyname),
        value,
        Some(comment),
        status,
    );
}

pub(crate) fn fits_write_key<T: KeyValue>(
    f: &mut Handle,
    keyname: &CStr,
    value: &T,
    comment: Option<&CStr>,
    status: &mut c_int,
) {
    api::fits_write_key(
        f.as_rsfitsio(),
        value.keyword(),
        chars(keyname),
        comment.map(chars),
        status,
    );
}

pub(crate) fn fits_write_key_str(
    f: &mut Handle,
    keyname: &CStr,
    value: &CStr,
    comment: Option<&CStr>,
    status: &mut c_int,
) {
    api::fits_write_key_str(
        f.as_rsfitsio(),
        chars(keyname),
        chars(value),
        comment.map(chars),
        status,
    );
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
    api::fits_read_img(
        f.as_rsfitsio(),
        datatype,
        firstelem,
        nelem,
        None,
        array,
        None,
        status,
    );
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
    api::fits_read_subset(
        f.as_rsfitsio(),
        datatype,
        fpixel,
        lpixel,
        inc,
        None,
        array,
        None,
        status,
    );
}

pub(crate) fn fits_write_img(
    f: &mut Handle,
    datatype: c_int,
    firstelem: i64,
    nelem: i64,
    array: &[u8],
    status: &mut c_int,
) {
    api::fits_write_img(f.as_rsfitsio(), datatype, firstelem, nelem, array, status);
}

pub(crate) fn fits_write_subset(
    f: &mut Handle,
    datatype: c_int,
    fpixel: &[c_long],
    lpixel: &[c_long],
    array: &[u8],
    status: &mut c_int,
) {
    api::fits_write_subset(f.as_rsfitsio(), datatype, fpixel, lpixel, array, status);
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
    api::fits_read_col(
        f.as_rsfitsio(),
        datatype,
        colnum,
        firstrow,
        firstelem,
        nelem,
        None,
        array,
        None,
        status,
    );
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
    let nelem = array.len() as i64;
    api::fits_read_col_log(
        f.as_rsfitsio(),
        colnum,
        firstrow,
        firstelem,
        nelem,
        nulval,
        array,
        None,
        status,
    );
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
    let mut bufs: Vec<&mut [c_char]> = array.iter_mut().map(|s| s.as_mut_slice()).collect();
    let nelem = bufs.len() as i64;
    api::fits_read_col_str(
        f.as_rsfitsio(),
        colnum,
        firstrow,
        firstelem,
        nelem,
        None,
        &mut bufs,
        None,
        status,
    );
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
    api::fits_write_col(
        f.as_rsfitsio(),
        datatype,
        colnum,
        firstrow,
        firstelem,
        nelem,
        array,
        status,
    );
}

pub(crate) fn fits_write_col_str(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    array: &[CString],
    status: &mut c_int,
) {
    let strs: Vec<&[c_char]> = array.iter().map(|s| chars(s)).collect();
    api::fits_write_col_str(
        f.as_rsfitsio(),
        colnum,
        firstrow,
        firstelem,
        strs.len() as i64,
        &strs,
        status,
    );
}

pub(crate) fn fits_get_col_display_width(
    f: &mut Handle,
    colnum: c_int,
    width: &mut c_int,
    status: &mut c_int,
) {
    api::fits_get_col_display_width(f.as_rsfitsio(), colnum, width, status);
}

pub(crate) fn fits_get_errstatus(status: c_int, text: &mut [c_char; FLEN_STATUS]) {
    api::fits_get_errstatus(status, text);
}
