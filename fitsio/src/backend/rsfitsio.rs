//! Backend calling the safe API of the pure Rust `rsfitsio` port of CFITSIO
//!
//! Unlike the C backend this needs no `unsafe`: `rsfitsio` takes references, slices and
//! `Option`s, and an open file is an owned `Box<fitsfile>`.

// C uses long argument lists
#![allow(clippy::too_many_arguments)]
#![forbid(unsafe_code)]

use super::{
    c_char, c_int, c_long, checked_subset_len, col_str_buffer_len, FLEN_COMMENT, FLEN_FILENAME,
    FLEN_STATUS, FLEN_VALUE,
};
use crate::types::HasFitsDataType;
use bytemuck::{cast_slice, cast_slice_mut};
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
    let mut code: [c_char; 3] = [0; 3];
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
    dtype[..3].copy_from_slice(&code);
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

/// Read `array.len()` pixels, starting at the 1-based `firstelem`
pub(crate) fn fits_read_img<T: HasFitsDataType>(
    f: &mut Handle,
    firstelem: i64,
    array: &mut [T],
    status: &mut c_int,
) {
    let nelem = array.len() as i64;
    api::fits_read_img(
        f.as_rsfitsio(),
        T::FITS_DATA_TYPE.into(),
        firstelem,
        nelem,
        None,
        cast_slice_mut(array),
        None,
        status,
    );
}

/// Read the region running from the 1-based `fpixel` to `lpixel` inclusive, stepping by `inc`
///
/// # Panics
///
/// If `array` is smaller than the region.
pub(crate) fn fits_read_subset<T: HasFitsDataType>(
    f: &mut Handle,
    fpixel: &[c_long],
    lpixel: &[c_long],
    inc: &[c_long],
    array: &mut [T],
    status: &mut c_int,
) {
    let len = match checked_subset_len(f, fpixel, lpixel, Some(inc), status) {
        Some(len) => len,
        None => return,
    };
    assert!(array.len() >= len, "array smaller than the image region");
    api::fits_read_subset(
        f.as_rsfitsio(),
        T::FITS_DATA_TYPE.into(),
        fpixel,
        lpixel,
        inc,
        None,
        cast_slice_mut(array),
        None,
        status,
    );
}

/// Write all of `array`, starting at the 1-based `firstelem`
pub(crate) fn fits_write_img<T: HasFitsDataType>(
    f: &mut Handle,
    firstelem: i64,
    array: &[T],
    status: &mut c_int,
) {
    api::fits_write_img(
        f.as_rsfitsio(),
        T::FITS_DATA_TYPE.into(),
        firstelem,
        array.len() as i64,
        cast_slice(array),
        status,
    );
}

/// Write the region running from the 1-based `fpixel` to `lpixel` inclusive
///
/// # Panics
///
/// If `array` is smaller than the region.
pub(crate) fn fits_write_subset<T: HasFitsDataType>(
    f: &mut Handle,
    fpixel: &[c_long],
    lpixel: &[c_long],
    array: &[T],
    status: &mut c_int,
) {
    let len = match checked_subset_len(f, fpixel, lpixel, None, status) {
        Some(len) => len,
        None => return,
    };
    assert!(array.len() >= len, "array smaller than the image region");
    api::fits_write_subset(
        f.as_rsfitsio(),
        T::FITS_DATA_TYPE.into(),
        fpixel,
        lpixel,
        cast_slice(array),
        status,
    );
}

/// Read `array.len()` values from a column, without checking for nulls
pub(crate) fn fits_read_col<T: HasFitsDataType>(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    array: &mut [T],
    status: &mut c_int,
) {
    let nelem = array.len() as i64;
    api::fits_read_col(
        f.as_rsfitsio(),
        T::FITS_DATA_TYPE.into(),
        colnum,
        firstrow,
        firstelem,
        nelem,
        None,
        cast_slice_mut(array),
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

/// Read `nelem` values of a column as strings, starting at the 1-based `firstrow`
///
/// Each returned buffer is nul-terminated.
pub(crate) fn fits_read_col_str(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    nelem: usize,
    status: &mut c_int,
) -> Vec<Vec<c_char>> {
    let len = match col_str_buffer_len(f, colnum, status) {
        Some(len) => len,
        None => return Vec::new(),
    };

    let mut array = vec![vec![0; len]; nelem];
    let mut bufs: Vec<&mut [c_char]> = array.iter_mut().map(|s| s.as_mut_slice()).collect();
    // An empty null value turns off null checking for string columns. rsfitsio, like CFITSIO,
    // needs somewhere to record nulls in logical and numeric columns.
    let nulval: [c_char; 1] = [0];
    let mut anynul = 0;
    api::fits_read_col_str(
        f.as_rsfitsio(),
        colnum,
        firstrow,
        1,
        nelem as i64,
        Some(&nulval),
        &mut bufs,
        Some(&mut anynul),
        status,
    );
    array
}

/// Write all of `array` to a column, starting at the 1-based `firstrow`
pub(crate) fn fits_write_col<T: HasFitsDataType>(
    f: &mut Handle,
    colnum: c_int,
    firstrow: i64,
    firstelem: i64,
    array: &[T],
    status: &mut c_int,
) {
    api::fits_write_col(
        f.as_rsfitsio(),
        T::FITS_DATA_TYPE.into(),
        colnum,
        firstrow,
        firstelem,
        array.len() as i64,
        cast_slice(array),
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

/// Fetch a column's datatype code (negative for variable-length columns), repeat count and width
pub(crate) fn fits_get_coltype(
    f: &mut Handle,
    colnum: c_int,
    typecode: &mut c_int,
    repeat: &mut i64,
    width: &mut i64,
    status: &mut c_int,
) {
    api::fits_get_coltypell(
        f.as_rsfitsio(),
        colnum,
        Some(typecode),
        Some(repeat),
        Some(width),
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
