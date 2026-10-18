//! [`FitsFile`](struct.FitsFile.html)

/* Depending on the architecture, different functions have to be called. For example arm systems
 * define `int` as 4 bytes, and `long` as 4 bytes, unlike x86_64 systems which define `long` types
 * as 8 bytes.
 *
 * In this case, we have to use `_longlong` cfitsio functions on arm architectures (and other
 * similar architectures).
 */

use crate::backend::{self, c_int, c_long, Handle, FLEN_FILENAME, FLEN_VALUE};
use crate::errors::{check_status, Error, Result};
use crate::hdu::{DescribesHdu, FitsHdu, FitsHduIterator, HduInfo};
use crate::images::{ImageDescription, ImageType};
use crate::stringutils::{self, status_to_string};
use crate::tables::{ColumnDataDescription, ConcreteColumnDescription};
use std::ffi;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Main entry point to the FITS file format
///
/// Dropping a `FitsFile` closes the file on disk, flushing existing buffers.
pub struct FitsFile {
    file_path: PathBuf,
    open_mode: FileOpenMode,
    pub(crate) fptr: Handle,
}

impl FitsFile {
    /**
    Open a fits file from disk

    # Example

    ```rust
    use fitsio::FitsFile;
    # use std::error::Error;

    # fn main() -> Result<(), Box<dyn Error>> {
    # let filename = "../testdata/full_example.fits";
    // let filename = ...;
    let fptr = FitsFile::open(filename)?;
    # Ok(())
    # }
    ```
    */
    pub fn open<T: AsRef<Path>>(filename: T) -> Result<Self> {
        let mut status = 0;
        let file_path = filename.as_ref();
        let filename = file_path.to_str().expect("converting filename");
        let c_filename = ffi::CString::new(filename)?;

        let fptr = Handle::open(&c_filename, FileOpenMode::READONLY as c_int, &mut status);

        check_status(status).map(|_| match fptr {
            Some(fptr) => FitsFile {
                fptr,
                open_mode: FileOpenMode::READONLY,
                file_path: file_path.to_path_buf(),
            },
            None => unimplemented!(),
        })
    }

    /**
    Open a fits file in read/write mode

    # Example

    ```rust
    # fn main() -> Result<(), Box<std::error::Error>> {
    # let filename = "../testdata/full_example.fits";
    use fitsio::FitsFile;

    // let filename = ...;
    let fptr = FitsFile::edit(filename)?;
    # Ok(())
    # }
    ```
    */
    pub fn edit<T: AsRef<Path>>(filename: T) -> Result<Self> {
        let mut status = 0;
        let file_path = filename.as_ref();
        let filename = file_path.to_str().expect("converting filename");
        let c_filename = ffi::CString::new(filename)?;

        let fptr = Handle::open(&c_filename, FileOpenMode::READWRITE as c_int, &mut status);

        check_status(status).map(|_| match fptr {
            Some(fptr) => FitsFile {
                fptr,
                open_mode: FileOpenMode::READWRITE,
                file_path: file_path.to_path_buf(),
            },
            None => unimplemented!(),
        })
    }

    /**
    Create a new fits file on disk

    The [`create`] method returns a [`NewFitsFile`], which is an
    internal representation of a temporary fits file on disk, before the file is fully created.

    This representation has two methods: [`open`] and [`with_custom_primary`]. The [`open`]
    method actually creates the file on disk, but before calling this method, the
    [`with_custom_primary`] method can be used to add a custom primary HDU. This is mostly
    useful for images. Otherwise, a default primary HDU is created.  An example of not adding a
    custom primary HDU is shown above. Below we see an example of [`with_custom_primary`]:

    # Example

    ```rust
    # fn main() -> Result<(), Box<std::error::Error>> {
    # let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
    # let tdir_path = tdir.path();
    # let filename = tdir_path.join("test.fits");
    use fitsio::FitsFile;
    use fitsio::images::{ImageDescription, ImageType};

    // let filename = ...;
    let description = ImageDescription {
        data_type: ImageType::Double,
        dimensions: &[52, 103],
    };

    let fptr = FitsFile::create(filename)
        .with_custom_primary(&description)
        .open()?;
    # Ok(())
    # }
    ```

    [`create`]: #method.create
    [`NewFitsFile`]: fitsfile/struct.NewFitsFile.html
    [`open`]: fitsfile/struct.NewFitsFile.html#method.open
    [`with_custom_primary`]: fitsfile/struct.NewFitsFile.html#method.with_custom_primary
    */
    pub fn create<'a, T: AsRef<Path>>(path: T) -> NewFitsFile<'a, T> {
        NewFitsFile {
            path,
            image_description: None,
            overwrite: false,
        }
    }

    /// Method to extract what open mode the file is in
    pub(crate) fn open_mode(&mut self) -> Result<FileOpenMode> {
        let mut status = 0;
        let mut iomode = 0;
        backend::fits_file_mode(&mut self.fptr, &mut iomode, &mut status);

        check_status(status).map(|_| match iomode {
            0 => FileOpenMode::READONLY,
            1 => FileOpenMode::READWRITE,
            _ => unreachable!(),
        })
    }

    fn add_empty_primary(&mut self) -> Result<()> {
        let mut status = 0;
        backend::fits_write_imghdr(
            &mut self.fptr,
            ImageType::UnsignedByte.into(),
            &[],
            &mut status,
        );

        check_status(status)
    }

    /// Change the current HDU
    pub(crate) fn change_hdu<T: DescribesHdu>(&mut self, hdu_description: T) -> Result<()> {
        hdu_description.change_hdu(self)
    }

    /**
    Return a new HDU object

    HDU information belongs to the [`FitsHdu`] object. HDUs can be fetched by `String`/`str` or
    integer (0-indexed).  The `HduInfo` object contains information about the current HDU:

    # Example

    ```rust
    # use fitsio::FitsFile;
    use fitsio::hdu::HduInfo;
    #
    # fn main() -> Result<(), Box<std::error::Error>> {
    # let filename = "../testdata/full_example.fits";
    # let mut fptr = FitsFile::open(filename)?;
    let hdu = fptr.hdu(0)?;
    // image HDU
    if let HduInfo::ImageInfo { shape, .. } = hdu.info {
       println!("Image is {}-dimensional", shape.len());
       println!("Found image with shape {:?}", shape);
    }
    # let hdu = fptr.hdu("TESTEXT")?;

    // tables
    if let HduInfo::TableInfo { column_descriptions, num_rows, .. } = hdu.info {
        println!("Table contains {} rows", num_rows);
        println!("Table has {} columns", column_descriptions.len());
    }
    # Ok(())
    # }
    ```

    [`FitsHdu`]: hdu/struct.FitsHdu.html
    */
    pub fn hdu<T: DescribesHdu>(&mut self, hdu_description: T) -> Result<FitsHdu> {
        FitsHdu::new(self, hdu_description)
    }

    /**
    Return the primary hdu (HDU 0)

    # Example

    ```rust
    # use fitsio::{FitsFile, hdu::HduInfo};
    #
    # fn main() -> Result<(), Box<std::error::Error>> {
    # let filename = "../testdata/full_example.fits";
    # let mut fptr = FitsFile::open(filename)?;
    let hdu = fptr.hdu(0)?;
    let phdu = fptr.primary_hdu()?;
    assert_eq!(hdu, phdu);
    # Ok(())
    # }
    ```
    */
    pub fn primary_hdu(&mut self) -> Result<FitsHdu> {
        self.hdu(0)
    }

    /// Return the file path of the file
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /**
    Get the number of HDUs in the file

    # Example

    ```rust
    # use fitsio::FitsFile;
    # fn main() -> Result<(), Box<dyn std::error::Error>> {
    # let filename = "../testdata/full_example.fits";
    # let mut fptr = FitsFile::open(filename)?;
    let num_hdus = fptr.num_hdus()?;
    assert_eq!(num_hdus, 2);
    # Ok(())
    # }
    ```
    */
    pub fn num_hdus(&mut self) -> Result<usize> {
        let mut status = 0;
        let mut num_hdus = 0;
        backend::fits_get_num_hdus(&mut self.fptr, &mut num_hdus, &mut status);

        check_status(status).map(|_| num_hdus as _)
    }

    /// Return the list of HDU names
    pub(crate) fn hdu_names(&mut self) -> Result<Vec<String>> {
        let num_hdus = self.num_hdus()?;
        let mut result = Vec::with_capacity(num_hdus);
        for i in 0..num_hdus {
            let hdu = self.hdu(i)?;
            let name = hdu.name(self)?;
            result.push(name);
        }
        Ok(result)
    }

    pub(crate) fn make_current(&mut self, hdu: &FitsHdu) -> Result<()> {
        self.change_hdu(hdu.number)
    }

    pub(crate) fn hdu_number(&mut self) -> usize {
        let hdu_num = backend::fits_get_hdu_num(&mut self.fptr);
        (hdu_num - 1) as usize
    }

    /// Get the current hdu as an HDU object
    pub(crate) fn current_hdu(&mut self) -> Result<FitsHdu> {
        let current_hdu_number = self.hdu_number();
        self.hdu(current_hdu_number)
    }

    /// Get the current hdu info
    pub(crate) fn fetch_hdu_info(&mut self) -> Result<HduInfo> {
        let mut status = 0;
        let mut hdu_type = 0;

        backend::fits_get_hdu_type(&mut self.fptr, &mut hdu_type, &mut status);

        let hdu_type = match hdu_type {
            0 => {
                let mut dimensions = 0;
                backend::fits_get_img_dim(&mut self.fptr, &mut dimensions, &mut status);

                let mut shape: Vec<c_long> = vec![0; dimensions as usize];
                backend::fits_get_img_size(&mut self.fptr, &mut shape, &mut status);

                /* Reverse the image dimensions to be more like the C convention */
                shape.reverse();

                let mut bitpix = 0;
                /* Use equiv type as this is more useful
                 *
                 * See description here:
                 * https://heasarc.gsfc.nasa.gov/docs/software/fitsio/c/c_user/node40.html
                 */
                backend::fits_get_img_equivtype(&mut self.fptr, &mut bitpix, &mut status);

                let image_type = match bitpix {
                    8 => ImageType::UnsignedByte,
                    10 => ImageType::Byte,
                    16 => ImageType::Short,
                    20 => ImageType::UnsignedShort,
                    32 => ImageType::Long,
                    40 => ImageType::UnsignedLong,
                    64 => ImageType::LongLong,
                    -32 => ImageType::Float,
                    -64 => ImageType::Double,
                    _ => unreachable!("{}", format!("Unhandled image bitpix type: {}", bitpix)),
                };

                HduInfo::ImageInfo {
                    shape: shape.iter().map(|v| *v as usize).collect(),
                    image_type,
                }
            }
            1 | 2 => {
                let mut num_rows = 0;
                backend::fits_get_num_rows(&mut self.fptr, &mut num_rows, &mut status);

                let mut num_cols = 0;
                backend::fits_get_num_cols(&mut self.fptr, &mut num_cols, &mut status);
                let mut column_descriptions = Vec::with_capacity(num_cols as usize);

                for i in 0..num_cols {
                    let mut name_buffer = [0; FLEN_VALUE];
                    let mut type_buffer = [0; FLEN_VALUE];
                    let mut repeats: c_long = 0;
                    backend::fits_get_bcolparms(
                        &mut self.fptr,
                        i + 1,
                        &mut name_buffer,
                        &mut type_buffer,
                        &mut repeats,
                        &mut status,
                    );
                    let mut col = ConcreteColumnDescription {
                        name: stringutils::buf_to_string(&name_buffer)?,
                        data_type: stringutils::buf_to_string(&type_buffer)?
                            .parse::<ColumnDataDescription>()?,
                    };
                    col.data_type.repeat = repeats as usize;
                    column_descriptions.push(col);
                }

                HduInfo::TableInfo {
                    column_descriptions,
                    num_rows: num_rows as usize,
                }
            }
            _ => panic!("Invalid hdu type found"),
        };

        check_status(status).map(|_| hdu_type)
    }

    /**
    Create a new fits table

    Create a new fits table, with columns as detailed in the [`ColumnDescription`] object.

    # Example

    ```rust
    use fitsio::tables::{ColumnDataType, ColumnDescription};

    # fn main() -> Result<(), Box<std::error::Error>> {
    # let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
    # let tdir_path = tdir.path();
    # let filename = tdir_path.join("test.fits");
    # let mut fptr = fitsio::FitsFile::create(filename).open()?;
    let first_description = ColumnDescription::new("A")
        .with_type(ColumnDataType::Int)
        .create()?;
    let second_description = ColumnDescription::new("B")
        .with_type(ColumnDataType::Long)
        .create()?;
    let descriptions = &[first_description, second_description];
    let hdu = fptr.create_table("EXTNAME".to_string(), descriptions)?;
    # Ok(())
    # }
    ```

    [`ColumnDescription`]: tables/struct.ColumnDescription.html
    */
    pub fn create_table<T>(
        &mut self,
        extname: T,
        table_description: &[ConcreteColumnDescription],
    ) -> Result<FitsHdu>
    where
        T: Into<String>,
    {
        fits_check_readwrite!(self);

        let tfields = table_description
            .iter()
            .map(|desc| ffi::CString::new(desc.name.clone()))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let ttype = table_description
            .iter()
            .map(|desc| ffi::CString::new(String::from(desc.clone().data_type)))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let c_extname = ffi::CString::new(extname.into())?;

        let hdu_info = HduInfo::TableInfo {
            column_descriptions: table_description.to_vec(),
            num_rows: 0,
        };

        let mut status: c_int = 0;
        backend::fits_create_tbl(
            &mut self.fptr,
            hdu_info.into(),
            0,
            &tfields,
            &ttype,
            Some(&c_extname),
            &mut status,
        );

        check_status(status).and_then(|_| self.current_hdu())
    }

    /**
    Create a new fits image, and return the [`FitsHdu`](hdu/struct.FitsHdu.html) object.

    This method takes an [`ImageDescription`] struct which defines the desired layout of the
    image HDU.

    # Example

    ```rust
    use fitsio::images::{ImageDescription, ImageType};

    # fn main() -> Result<(), Box<std::error::Error>> {
    # let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
    # let tdir_path = tdir.path();
    # let filename = tdir_path.join("test.fits");
    # let mut fptr = fitsio::FitsFile::create(filename).open()?;
    let image_description = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[100, 100],
    };
    let hdu = fptr.create_image("EXTNAME".to_string(), &image_description)?;
    # Ok(())
    # }
    ```

    [`ImageDescription`]: images/struct.ImageDescription.html
    */
    pub fn create_image<T>(
        &mut self,
        extname: T,
        image_description: &ImageDescription,
    ) -> Result<FitsHdu>
    where
        T: Into<String>,
    {
        fits_check_readwrite!(self);

        let mut status = 0;

        if status != 0 {
            return Err(FitsError {
                status,
                // unwrap guaranteed to succesed as status > 0
                message: status_to_string(status)?.unwrap(),
            }
            .into());
        }

        let mut dimensions: Vec<c_long> = image_description
            .dimensions
            .iter()
            .map(|d| *d as c_long)
            .collect();
        dimensions.reverse();

        backend::fits_create_img(
            &mut self.fptr,
            image_description.data_type.into(),
            &dimensions,
            &mut status,
        );

        if status != 0 {
            return Err(FitsError {
                status,
                // unwrap guaranteed to succesed as status > 0
                message: status_to_string(status)?.unwrap(),
            }
            .into());
        }

        // Current HDU should be at the new HDU
        let current_hdu = self.current_hdu()?;
        current_hdu.write_key(self, "EXTNAME", extname.into())?;

        check_status(status).and_then(|_| self.current_hdu())
    }

    /**
    Iterate over the HDUs in the file

    # Example

    ```rust
    # fn main() -> Result<(), Box<std::error::Error>> {
    #     let mut fptr = fitsio::FitsFile::open("../testdata/full_example.fits")?;
    for hdu in fptr.iter() {
        // Do something with hdu
    }
    # Ok(())
    # }
    ```
    */
    pub fn iter(&mut self) -> FitsHduIterator<'_> {
        FitsHduIterator {
            current: 0,
            max: self.num_hdus().unwrap(),
            fits_file: self,
        }
    }

    /**
    Pretty-print file to stdout

    Fits files can be pretty-printed with [`pretty_print`], or its more powerful
    cousin [`pretty_write`].

    # Example

    ```rust
    # fn main() -> Result<(), Box<std::error::Error>> {
    use fitsio::FitsFile;

    # let filename = "../testdata/full_example.fits";
    # use std::io;
    let mut fptr = FitsFile::open(filename)?;
    fptr.pretty_print()?;
    // or
    fptr.pretty_write(&mut io::stdout())?;
    # Ok(())
    # }
    ```

    [`pretty_print`]: #method.pretty_print
    [`pretty_write`]: #method.pretty_write
    */
    pub fn pretty_print(&mut self) -> Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();

        self.pretty_write(&mut handle)
    }

    /**
    Pretty-print the fits file structure to any `Write` implementor

    Fits files can be pretty-printed with [`pretty_print`], or its more powerful
    cousin [`pretty_write`].

    # Example

    ```rust
    # fn main() -> Result<(), Box<std::error::Error>> {
    use fitsio::FitsFile;

    # let filename = "../testdata/full_example.fits";
    # use std::io;
    let mut fptr = FitsFile::open(filename)?;
    fptr.pretty_print()?;
    // or
    fptr.pretty_write(&mut io::stdout())?;
    # Ok(())
    # }
    ```

    [`pretty_print`]: #method.pretty_print
    [`pretty_write`]: #method.pretty_write
    */
    pub fn pretty_write<W>(&mut self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        writeln!(w, "\n  file: {}", self.file_path.display())?;
        match self.open_mode {
            FileOpenMode::READONLY => writeln!(w, "  mode: READONLY")?,
            FileOpenMode::READWRITE => writeln!(w, "  mode: READWRITE")?,
        };

        /* Header line for HDUs */
        writeln!(w, "  extnum hdutype      hduname    details")?;

        let hdu_names = self.hdu_names().expect("fetching hdu names");

        for (i, hdu) in self.iter().enumerate() {
            let hdu_name = &hdu_names[i];

            match hdu.info {
                HduInfo::ImageInfo { shape, image_type } => {
                    let hdu_type = "IMAGE_HDU";
                    writeln!(
                        w,
                        "  {extnum:<6} {hdu_type:12} {hdu_name:10} dimensions: {dimensions:?}, type: {image_type:?}",
                        extnum = i,
                        hdu_type = hdu_type,
                        hdu_name = hdu_name,
                        dimensions = shape,
                        image_type = image_type,
                    )?;
                }
                HduInfo::TableInfo {
                    column_descriptions,
                    num_rows,
                } => {
                    let hdu_type = "BINARY_TBL";
                    writeln!(
                        w,
                        "  {extnum:<6} {hdu_type:12} {hdu_name:10} num_cols: {num_cols}, num_rows: {num_rows}",
                        extnum = i,
                        hdu_type = hdu_type,
                        hdu_name = hdu_name,
                        num_cols = column_descriptions.len(),
                        num_rows = num_rows,
                    )?;
                }
                HduInfo::AnyInfo => unreachable!(),
            }
        }

        Ok(())
    }

    #[cfg(feature = "cfitsio")]
    /// Return a pointer to the underlying C `fitsfile` object representing the current file.
    ///
    /// Any changes to the underlying fits file will not be updated in existing [`FitsHdu`]
    /// objects, so these must be recreated.
    ///
    /// # Safety
    ///
    /// This is marked as `unsafe` as it is definitely something that is not required by most
    /// users, and hence the unsafe-ness marks it as an advanced feature. I have also not
    /// considered possible concurrency or data race issues as yet.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fitsio::FitsFile;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let filename = "../testdata/full_example.fits";
    /// let mut fptr = FitsFile::open(filename)?;
    ///
    /// /* Find out the number of HDUs in the file */
    /// let mut num_hdus = 0;
    /// let mut status = 0;
    ///
    /// unsafe {
    ///     let fitsfile = fptr.as_raw();
    ///
    ///     /* Use the unsafe fitsio-sys low level library to call a function that is possibly not
    ///     implemented in this crate */
    ///     fitsio_sys::ffthdu(fitsfile, &mut num_hdus, &mut status);
    /// }
    /// assert_eq!(num_hdus, 2);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`FitsHdu`]: hdu/struct.FitsHdu.html
    pub unsafe fn as_raw(&mut self) -> *mut crate::sys::fitsfile {
        self.fptr.as_raw()
    }

    #[cfg(feature = "cfitsio")]
    /// Load a `FitsFile` from a `fitsio_sys::fitsfile` pointer.
    ///
    /// # Safety
    ///
    /// This constructor is inherently unsafe - the Rust compiler cannot verify the validity of
    /// the pointer supplied. It is therefore the responsibility of the caller to prove that the
    /// pointer is a valid `fitsfile` by calling an `unsafe` function.
    ///
    /// it is up to the caller to guarantee that the pointer given was
    ///
    /// 1. created by `cfitsio` (or [`fitsio_sys`]), and
    /// 2. it represents a valid FITS file.
    ///
    /// Given these two things, a [`FitsFile`] can be created.
    ///
    /// On success the [`FitsFile`] owns the pointer and closes it when dropped. On error the
    /// pointer is left open and the caller still owns it.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fitsio::{sys::ffopen, FileOpenMode, FitsFile};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let filename = "../testdata/full_example.fits";
    /// let mut fptr = std::ptr::null_mut();
    /// let mut status = 0;
    /// let c_filename = std::ffi::CString::new(filename).expect("filename is not a valid C-string");
    ///
    /// unsafe {
    ///     ffopen(
    ///         &mut fptr as *mut *mut _,
    ///         c_filename.as_ptr(),
    ///         0, // readonly
    ///         &mut status,
    ///     );
    /// }
    /// assert_eq!(status, 0);
    ///
    /// let mut f = unsafe { FitsFile::from_raw(fptr, FileOpenMode::READONLY) }.unwrap();
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn from_raw(
        fptr: *mut crate::sys::fitsfile,
        mode: FileOpenMode,
    ) -> Result<FitsFile> {
        let fptr = std::ptr::NonNull::new(fptr).ok_or(Error::NullPointer)?;
        let mut fptr = Handle::from_raw(fptr);
        match file_path(&mut fptr) {
            Ok(file_path) => Ok(Self {
                file_path,
                open_mode: mode,
                fptr,
            }),
            Err(e) => {
                // the caller still owns the file, so it must not be closed
                fptr.into_raw();
                Err(e)
            }
        }
    }

    /// Borrow the underlying `rsfitsio` file, to call `rsfitsio` routines this crate does not
    /// wrap.
    ///
    /// `fitsio` does not re-export `rsfitsio`: add it to your own dependencies, at a version
    /// compatible with the one `fitsio` uses, to call its routines.
    ///
    /// Any changes to the underlying fits file will not be updated in existing [`FitsHdu`]
    /// objects, so these must be recreated.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fitsio::FitsFile;
    /// use rsfitsio::aliases::rust_api::fits_get_num_hdus;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let filename = "../testdata/full_example.fits";
    /// let mut f = FitsFile::open(filename)?;
    ///
    /// let mut num_hdus = 0;
    /// let mut status = 0;
    /// fits_get_num_hdus(f.as_rsfitsio(), &mut num_hdus, &mut status);
    /// assert_eq!(num_hdus, 2);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`FitsHdu`]: hdu/struct.FitsHdu.html
    #[cfg(feature = "rsfitsio-experimental")]
    pub fn as_rsfitsio(&mut self) -> &mut rsfitsio::fitsio::fitsfile {
        self.fptr.as_rsfitsio()
    }

    /// Create a `FitsFile` from a file opened through `rsfitsio`, for example one opened from
    /// memory.
    ///
    /// `fitsio` does not re-export `rsfitsio`: add it to your own dependencies, at a version
    /// compatible with the one `fitsio` uses, to call its routines.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fitsio::{FileOpenMode, FitsFile};
    /// use rsfitsio::aliases::rust_api::fits_open_file;
    /// use rsfitsio::fitsio::READONLY;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let name = std::ffi::CString::new("../testdata/full_example.fits")?;
    /// let mut fptr = None;
    /// let mut status = 0;
    /// fits_open_file(&mut fptr, bytemuck::cast_slice(name.as_bytes_with_nul()), READONLY, &mut status);
    /// assert_eq!(status, 0);
    ///
    /// let mut f = FitsFile::from_rsfitsio(fptr.unwrap(), FileOpenMode::READONLY)?;
    /// assert_eq!(f.num_hdus()?, 2);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "rsfitsio-experimental")]
    pub fn from_rsfitsio(
        fptr: Box<rsfitsio::fitsio::fitsfile>,
        mode: FileOpenMode,
    ) -> Result<FitsFile> {
        let mut fptr = Handle::from_rsfitsio(fptr);
        Ok(Self {
            file_path: file_path(&mut fptr)?,
            open_mode: mode,
            fptr,
        })
    }
}

/// The path of an open file
fn file_path(fptr: &mut Handle) -> Result<PathBuf> {
    let mut buf = [0; FLEN_FILENAME];
    let mut status = 0;
    backend::fits_file_name(fptr, &mut buf, &mut status);
    check_status(status)?;
    Ok(PathBuf::from(stringutils::buf_to_string(&buf)?))
}

/**
New fits file representation

This is a temporary struct, which describes how the primary HDU of a new file should be
created. It uses the builder pattern.

The [`with_custom_primary`][new-fits-file-with-custom-primary] method allows for creation of a
custom primary HDU.

# Example

```rust
# let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
# let tdir_path = tdir.path();
# let _filename = tdir_path.join("test.fits");
# let filename = _filename.to_str().unwrap();
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

// let filename = ...;
let description = ImageDescription {
    data_type: ImageType::Double,
    dimensions: &[52, 103],
};
let fptr = FitsFile::create(filename)
    .with_custom_primary(&description)
    .open()
    .unwrap();
```

The [`open`][new-fits-file-open] method actually creates a `Result<FitsFile>` from this
temporary representation.

# Example

```rust
# let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
# let tdir_path = tdir.path();
# let _filename = tdir_path.join("test.fits");
# let filename = _filename.to_str().unwrap();
use fitsio::FitsFile;

// let filename = ...;
let fptr = FitsFile::create(filename).open().unwrap();
```
[new-fits-file]: struct.NewFitsFile.html
[new-fits-file-open]: struct.NewFitsFile.html#method.open
[new-fits-file-with-custom-primary]: struct.NewFitsFile.html#method.with_custom_primary
*/
pub struct NewFitsFile<'a, T>
where
    T: AsRef<Path>,
{
    path: T,
    image_description: Option<ImageDescription<'a>>,
    overwrite: bool,
}

impl<'a, T> NewFitsFile<'a, T>
where
    T: AsRef<Path>,
{
    /**
    Create a `Result<FitsFile>` from a temporary [`NewFitsFile`][new-fits-file] representation.

    [new-fits-file]: struct.NewFitsFile.html
    */
    pub fn open(self) -> Result<FitsFile> {
        let mut status = 0;
        let file_path = self.path.as_ref();
        let path = file_path.to_str().expect("converting filename");
        let c_filename = ffi::CString::new(path)?;

        // Check if there is an existing file already with the given filename
        if self.path.as_ref().is_file() {
            // Check if the overwrite flag is set
            if !self.overwrite {
                return Err(Error::ExistingFile(path.to_owned()));
            } else {
                ::std::fs::remove_file(self.path.as_ref())?;
            }
        }

        let fptr = Handle::create(&c_filename, &mut status);

        check_status(status).and_then(|_| {
            let mut f = match fptr {
                Some(fptr) => FitsFile {
                    fptr,
                    open_mode: FileOpenMode::READWRITE,
                    file_path: file_path.to_path_buf(),
                },
                None => unimplemented!(),
            };

            match self.image_description {
                Some(ref description) => {
                    let _ = f.create_image("_PRIMARY".to_string(), description)?;
                }
                None => f.add_empty_primary()?,
            }
            Ok(f)
        })
    }

    /**
    When creating a new file, add a custom primary HDU description before creating the
    [`FitsFile`] object.

    # Example

    ```rust
    # fn main() -> Result<(), Box<dyn std::error::Error>> {
    # let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
    # let tdir_path = tdir.path();
    # let filename = tdir_path.join("test.fits");
    use fitsio::FitsFile;
    use fitsio::images::{ImageType, ImageDescription};

    // let filename = ...;
    let description = ImageDescription {
        data_type: ImageType::Double,
        dimensions: &[52, 103],
    };

    let fptr = FitsFile::create(filename)
        .with_custom_primary(&description)
        .open()?;
    # Ok(())
    # }
    ```

    [`FitsFile`]: struct.FitsFile.html
    */
    pub fn with_custom_primary(mut self, description: &ImageDescription<'a>) -> Self {
        self.image_description = Some(description.clone());
        self
    }

    /**
    Overwrite any existing files

    When creating a new fits file, if a file exists with the same filename, then overwrite the
    existing file. This does not however check if the underlying object is a file or not; it
    just removes it. For example, if the underlying "file" is a directory, this will fail to
    remove it.

    If this is not given, then when calling [`open`] will return an
    [`Error::ExistingFile(filename)`]

    # Example

    ```rust
    # fn main() -> Result<(), Box<std::error::Error>> {
    # let tdir = tempfile::Builder::new().prefix("fitsio-").tempdir().unwrap();
    # let tdir_path = tdir.path();
    # let filename = tdir_path.join("test.fits");
    use fitsio::FitsFile;

    // filename already exists
    let fptr = FitsFile::create(filename)
        .overwrite()
        .open()?;
    # Ok(())
    # }
    ```

    [`open`]: struct.NewFitsFile.html#method.open
    [`Error::ExistingFile(filename)`]: ../errors/enum.Error.html#variant.ExistingFile
    */
    pub fn overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }
}

/// Enumeration of file open modes
#[allow(missing_docs, clippy::upper_case_acronyms)]
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum FileOpenMode {
    READONLY,
    READWRITE,
}

macro_rules! fileopenmode_into_impl {
    ($t:ty) => {
        impl From<FileOpenMode> for $t {
            fn from(original: FileOpenMode) -> $t {
                match original {
                    FileOpenMode::READONLY => 0,
                    FileOpenMode::READWRITE => 1,
                }
            }
        }
    };
}

fileopenmode_into_impl!(u8);
fileopenmode_into_impl!(u32);
fileopenmode_into_impl!(u64);
fileopenmode_into_impl!(i8);
fileopenmode_into_impl!(i32);
fileopenmode_into_impl!(i64);

/// Enumeration of options for case sensitivity
#[allow(missing_docs, clippy::upper_case_acronyms)]
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum CaseSensitivity {
    CASEINSEN,
    CASESEN,
}

macro_rules! casesensitivity_into_impl {
    ($t:ty) => {
        impl From<CaseSensitivity> for $t {
            fn from(original: CaseSensitivity) -> $t {
                match original {
                    CaseSensitivity::CASEINSEN => 0,
                    CaseSensitivity::CASESEN => 1,
                }
            }
        }
    };
}

casesensitivity_into_impl!(u8);
casesensitivity_into_impl!(u32);
casesensitivity_into_impl!(u64);
casesensitivity_into_impl!(i8);
casesensitivity_into_impl!(i32);
casesensitivity_into_impl!(i64);

#[cfg(test)]
mod test {
    use std::os::raw::c_int;

    use crate::errors::Error;
    use crate::fitsfile::FitsFile;
    use crate::fitsfile::{FileOpenMode, ImageDescription};
    use crate::hdu::{FitsHdu, HduInfo};
    use crate::images::ImageType;
    use crate::tables::{ColumnDataType, ColumnDescription};
    use crate::testhelpers::{duplicate_test_file, with_temp_file};
    use std::ffi::CString;
    use std::path::Path;

    #[test]
    fn test_opening_an_existing_file() {
        match FitsFile::open("../testdata/full_example.fits") {
            Ok(_) => {}
            Err(e) => panic!("{:?}", e),
        }
    }

    #[test]
    fn test_creating_a_new_file() {
        with_temp_file(|filename| {
            FitsFile::create(filename)
                .open()
                .map(|mut f| {
                    assert!(Path::new(filename).exists());

                    // Ensure the empty primary has been written
                    let hdu = f.hdu(0).unwrap();
                    let naxis: i64 = hdu.read_key(&mut f, "NAXIS").unwrap();
                    assert_eq!(naxis, 0);
                })
                .unwrap();
        });
    }

    #[test]
    fn test_create_custom_primary_hdu() {
        with_temp_file(|filename| {
            {
                let description = ImageDescription {
                    data_type: ImageType::Double,
                    dimensions: &[100, 103],
                };
                FitsFile::create(filename)
                    .with_custom_primary(&description)
                    .open()
                    .unwrap();
            }
            let mut f = FitsFile::open(filename).unwrap();
            let hdu = f.hdu(0).unwrap();
            match hdu.info {
                HduInfo::ImageInfo { shape, image_type } => {
                    assert_eq!(shape, vec![100, 103]);
                    assert_eq!(image_type, ImageType::Double);
                }
                _ => panic!("INVALID"),
            }
        });
    }

    #[test]
    fn test_overwriting() {
        use std::fs::File;
        use std::io::Write;
        with_temp_file(|filename| {
            {
                // Test with existing file
                let mut f = File::create(filename).unwrap();
                f.write_all(b"Hello world").unwrap();
            }

            match FitsFile::create(filename).open() {
                Err(Error::ExistingFile(_)) => {}
                _ => unreachable!(),
            }
        });

        with_temp_file(|filename| {
            {
                // Test with existing file
                let mut f = File::create(filename).unwrap();
                f.write_all(b"Hello world").unwrap();
            }

            FitsFile::create(filename).overwrite().open().unwrap();
        });
    }

    #[test]
    fn test_cannot_write_to_readonly_file() {
        duplicate_test_file(|filename| {
            let mut f = FitsFile::open(filename).unwrap();

            match f.create_image(
                "FOO".to_string(),
                &ImageDescription {
                    data_type: ImageType::Long,
                    dimensions: &[100, 100],
                },
            ) {
                Err(Error::Fits(e)) => {
                    assert_eq!(e.status, 602);
                }
                _ => panic!("Should fail"),
            }

            let bar_column_description = ColumnDescription::new("bar")
                .with_type(ColumnDataType::Int)
                .create()
                .unwrap();
            match f.create_table("FOO".to_string(), &[bar_column_description]) {
                Err(Error::Fits(e)) => {
                    assert_eq!(e.status, 602);
                }
                _ => panic!("Should fail"),
            }
        });
    }

    #[test]
    fn test_editing_a_current_file() {
        duplicate_test_file(|filename| {
            {
                let mut f = FitsFile::edit(filename).unwrap();
                let image_hdu = f.hdu(0).unwrap();

                let data_to_write: Vec<i64> = (0..100).map(|_| 10101).collect();
                image_hdu
                    .write_section(&mut f, 0, 100, &data_to_write)
                    .unwrap();
            }

            {
                let mut f = FitsFile::open(filename).unwrap();
                let hdu = f.hdu(0).unwrap();
                let read_data: Vec<i64> = hdu.read_section(&mut f, 0, 10).unwrap();
                assert_eq!(read_data, vec![10101; 10]);
            }
        });
    }

    #[test]
    fn test_fetching_a_hdu() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        for i in 0..2 {
            f.change_hdu(i).unwrap();
            assert_eq!(f.hdu_number(), i);
        }

        match f.change_hdu(2) {
            Err(Error::Fits(e)) => assert_eq!(e.status, 107),
            _ => panic!("Error checking for failure"),
        }

        f.change_hdu("TESTEXT").unwrap();
        assert_eq!(f.hdu_number(), 1);
    }

    #[test]
    fn test_fetching_hdu_info() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        match f.fetch_hdu_info() {
            Ok(HduInfo::ImageInfo { shape, image_type }) => {
                assert_eq!(shape.len(), 2);
                assert_eq!(shape, vec![100, 100]);
                assert_eq!(image_type, ImageType::Long);
            }
            Err(e) => panic!("Error fetching hdu info {:?}", e),
            _ => panic!("Unknown error"),
        }

        f.change_hdu(1).unwrap();
        match f.fetch_hdu_info() {
            Ok(HduInfo::TableInfo {
                column_descriptions,
                num_rows,
            }) => {
                assert_eq!(num_rows, 50);
                assert_eq!(
                    column_descriptions
                        .iter()
                        .map(|desc| desc.name.clone())
                        .collect::<Vec<String>>(),
                    vec![
                        "intcol".to_string(),
                        "floatcol".to_string(),
                        "doublecol".to_string(),
                        "strcol".to_string(),
                    ]
                );
                assert_eq!(
                    column_descriptions
                        .iter()
                        .map(|desc| desc.data_type.typ)
                        .collect::<Vec<ColumnDataType>>(),
                    vec![
                        ColumnDataType::Int,
                        ColumnDataType::Float,
                        ColumnDataType::Double,
                        ColumnDataType::String,
                    ]
                );
            }
            Err(e) => panic!("Error fetching hdu info {:?}", e),
            _ => panic!("Unknown error"),
        }
    }

    #[test]
    fn test_getting_file_open_mode() {
        duplicate_test_file(|filename| {
            let mut f = FitsFile::open(filename).unwrap();
            assert_eq!(f.open_mode().unwrap(), FileOpenMode::READONLY);
        });

        duplicate_test_file(|filename| {
            let mut f = FitsFile::edit(filename).unwrap();
            assert_eq!(f.open_mode().unwrap(), FileOpenMode::READWRITE);
        });
    }

    #[test]
    fn test_adding_new_table() {
        with_temp_file(|filename| {
            {
                let mut f = FitsFile::create(filename).open().unwrap();
                let table_description = vec![ColumnDescription::new("bar")
                    .with_type(ColumnDataType::Int)
                    .create()
                    .unwrap()];
                f.create_table("foo".to_string(), &table_description)
                    .unwrap();
            }

            FitsFile::open(filename)
                .map(|mut f| {
                    f.change_hdu("foo").unwrap();
                    match f.fetch_hdu_info() {
                        Ok(HduInfo::TableInfo {
                            column_descriptions,
                            ..
                        }) => {
                            let column_names = column_descriptions
                                .iter()
                                .map(|desc| desc.name.clone())
                                .collect::<Vec<String>>();
                            let column_types = column_descriptions
                                .iter()
                                .map(|desc| desc.data_type.typ)
                                .collect::<Vec<_>>();
                            assert_eq!(column_names, vec!["bar".to_string()]);
                            assert_eq!(column_types, vec![ColumnDataType::Int]);
                        }
                        thing => panic!("{:?}", thing),
                    }
                })
                .unwrap();
        });
    }

    #[test]
    fn test_adding_new_image() {
        with_temp_file(|filename| {
            {
                let mut f = FitsFile::create(filename).open().unwrap();
                let image_description = ImageDescription {
                    data_type: ImageType::Long,
                    dimensions: &[100, 20],
                };
                f.create_image("foo".to_string(), &image_description)
                    .unwrap();
            }

            FitsFile::open(filename)
                .map(|mut f| {
                    f.change_hdu("foo").unwrap();
                    match f.fetch_hdu_info() {
                        Ok(HduInfo::ImageInfo { shape, .. }) => {
                            assert_eq!(shape, vec![100, 20]);
                        }
                        thing => panic!("{:?}", thing),
                    }
                })
                .unwrap();
        });
    }

    #[test]
    fn test_multidimensional_images() {
        with_temp_file(|filename| {
            let dimensions = [10, 20, 15];

            {
                let mut f = FitsFile::create(filename).open().unwrap();
                let image_description = ImageDescription {
                    data_type: ImageType::Long,
                    dimensions: &dimensions,
                };
                let image_hdu = f
                    .create_image("foo".to_string(), &image_description)
                    .unwrap();
                let data_to_write: Vec<i64> = (0..3000).collect();

                let xcoord = 0..dimensions[0] - 1;
                let ycoord = 0..dimensions[1] - 1;
                let zcoord = 0..dimensions[2] - 1;

                image_hdu
                    .write_region(&mut f, &[&xcoord, &ycoord, &zcoord], &data_to_write)
                    .unwrap();
            }

            let mut f = FitsFile::open(filename).unwrap();
            let hdu = f.hdu("foo").unwrap();

            let xcoord = 2..6;
            let ycoord = 11..17;
            let zcoord = 3..7;

            let read_data: Vec<i64> = hdu
                .read_region(&mut f, &[&xcoord, &ycoord, &zcoord])
                .unwrap();

            assert_eq!(read_data.len(), (6 - 2) * (17 - 11) * (7 - 3));
            assert_eq!(read_data[0], 614);
            assert_eq!(read_data[50], 958);
        });
    }

    #[test]
    fn test_fetching_hdu_object_hdu_info() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        let testext = f.hdu("TESTEXT").unwrap();
        match testext.info {
            HduInfo::TableInfo { num_rows, .. } => {
                assert_eq!(num_rows, 50);
            }
            _ => panic!("Incorrect HDU type found"),
        }
    }

    #[test]
    fn test_fetch_current_hdu() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        f.change_hdu("TESTEXT").unwrap();
        let hdu = f.current_hdu().unwrap();

        assert_eq!(
            hdu.read_key::<String>(&mut f, "EXTNAME").unwrap(),
            "TESTEXT".to_string()
        );
    }

    #[test]
    fn test_fetch_primary_hdu() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        let _hdu = f.primary_hdu().unwrap();
        assert_eq!(f.hdu_number(), 0);
    }

    #[test]
    fn test_fetch_number_of_hdus() {
        duplicate_test_file(|filename| {
            let mut f = FitsFile::open(filename).unwrap();
            let num_hdus = f.num_hdus().unwrap();
            assert_eq!(num_hdus, 2);
        });
    }

    #[test]
    fn test_fetch_hdu_names() {
        duplicate_test_file(|filename| {
            let mut f = FitsFile::open(filename).unwrap();
            let hdu_names = f.hdu_names().unwrap();
            assert_eq!(hdu_names.as_slice(), &["", "TESTEXT"]);
        });
    }

    #[test]
    fn test_creating_new_image_returns_hdu_object() {
        with_temp_file(|filename| {
            let mut f = FitsFile::create(filename).open().unwrap();
            let image_description = ImageDescription {
                data_type: ImageType::Long,
                dimensions: &[100, 20],
            };
            let hdu: FitsHdu = f
                .create_image("foo".to_string(), &image_description)
                .unwrap();
            assert_eq!(
                hdu.read_key::<String>(&mut f, "EXTNAME").unwrap(),
                "foo".to_string()
            );
        });
    }

    #[test]
    fn test_creating_new_table_returns_hdu_object() {
        with_temp_file(|filename| {
            let mut f = FitsFile::create(filename).open().unwrap();
            let table_description = vec![ColumnDescription::new("bar")
                .with_type(ColumnDataType::Int)
                .create()
                .unwrap()];
            let hdu: FitsHdu = f
                .create_table("foo".to_string(), &table_description)
                .unwrap();
            assert_eq!(
                hdu.read_key::<String>(&mut f, "EXTNAME").unwrap(),
                "foo".to_string()
            );
        });
    }

    #[test]
    fn test_cannot_write_column_to_image_hdu() {
        with_temp_file(|filename| {
            let data_to_write: Vec<i32> = vec![10101; 10];

            let mut f = FitsFile::create(filename).open().unwrap();

            let image_description = ImageDescription {
                data_type: ImageType::Long,
                dimensions: &[100, 20],
            };
            let hdu = f
                .create_image("foo".to_string(), &image_description)
                .unwrap();

            match hdu.write_col(&mut f, "bar", &data_to_write) {
                Err(Error::Message(msg)) => {
                    assert_eq!(msg, "Cannot write column data to FITS image")
                }
                _ => panic!("Should return an error"),
            }
        });
    }

    #[test]
    fn test_read_image_region_from_table() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        let hdu = f.hdu("TESTEXT").unwrap();
        match hdu.read_region::<Vec<i32>>(&mut f, &[&(0..10), &(0..10)]) {
            Err(Error::Message(msg)) => {
                assert!(msg.contains("cannot read image data from a table hdu"))
            }
            _ => panic!("SHOULD FAIL"),
        }
    }

    #[test]
    fn test_read_image_section_from_table() {
        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        let hdu = f.hdu("TESTEXT").unwrap();
        if let Err(Error::Message(msg)) = hdu.read_section::<Vec<i32>>(&mut f, 0, 100) {
            assert!(msg.contains("cannot read image data from a table hdu"));
        } else {
            panic!("Should have been an error");
        }
    }

    #[test]
    fn test_write_image_section_to_table() {
        with_temp_file(|filename| {
            let data_to_write: Vec<i64> = (0..100).map(|v| v + 50).collect();

            let mut f = FitsFile::create(filename).open().unwrap();
            let table_description = &[ColumnDescription::new("bar")
                .with_type(ColumnDataType::Int)
                .create()
                .unwrap()];
            let hdu = f
                .create_table("foo".to_string(), table_description)
                .unwrap();
            if let Err(Error::Message(msg)) = hdu.write_section(&mut f, 0, 100, &data_to_write) {
                assert_eq!(msg, "cannot write image data to a table hdu");
            } else {
                panic!("Should have thrown an error");
            }
        });
    }

    #[test]
    fn test_write_image_region_to_table() {
        with_temp_file(|filename| {
            let data_to_write: Vec<i64> = (0..100).map(|v| v + 50).collect();

            let mut f = FitsFile::create(filename).open().unwrap();
            let table_description = &[ColumnDescription::new("bar")
                .with_type(ColumnDataType::Int)
                .create()
                .unwrap()];
            let hdu = f
                .create_table("foo".to_string(), table_description)
                .unwrap();

            let ranges = vec![&(0..10), &(0..10)];
            if let Err(Error::Message(msg)) = hdu.write_region(&mut f, &ranges, &data_to_write) {
                assert_eq!(msg, "cannot write image data to a table hdu");
            } else {
                panic!("Should have thrown an error");
            }
        });
    }

    #[cfg(feature = "cfitsio")]
    #[test]
    fn test_access_fptr_unsafe() {
        use crate::sys::fitsfile;

        let mut f = FitsFile::open("../testdata/full_example.fits").unwrap();
        let fptr: *const fitsfile = unsafe { f.as_raw() };
        assert!(!fptr.is_null());
    }

    #[test]
    fn test_extended_filename_syntax() {
        let filename = "../testdata/full_example.fits[TESTEXT]";
        let mut f = FitsFile::open(filename).unwrap();
        match f.fetch_hdu_info() {
            Ok(HduInfo::TableInfo { .. }) => {}
            Ok(HduInfo::ImageInfo { .. }) => panic!("Should be binary table"),
            _ => panic!("ERROR!"),
        }
    }

    #[test]
    fn test_copy_hdu() {
        duplicate_test_file(|src_filename| {
            with_temp_file(|dest_filename| {
                let mut src = FitsFile::open(src_filename).unwrap();
                let src_hdu = src.hdu("TESTEXT").unwrap();

                {
                    let mut dest = FitsFile::create(dest_filename).open().unwrap();
                    src_hdu.copy_to(&mut src, &mut dest).unwrap();
                }

                let mut dest = FitsFile::open(dest_filename).unwrap();
                let _dest_hdu = dest.hdu("TESTEXT").unwrap();

                /* If we do not error then the hdu has been copied */
            });
        });
    }

    #[test]
    fn test_changing_image_returns_new_hdu() {
        duplicate_test_file(|filename| {
            let mut f = FitsFile::edit(filename).unwrap();
            let hdu = f.hdu(0).unwrap();
            let newhdu = hdu.resize(&mut f, &[1024, 1024]).unwrap();

            match newhdu.info {
                HduInfo::ImageInfo { shape, .. } => {
                    assert_eq!(shape, vec![1024, 1024]);
                }
                _ => panic!("ERROR!"),
            }
        });
    }

    #[cfg(feature = "cfitsio")]
    #[test]
    fn test_get_filename() {
        use crate::sys::{ffopen, fitsfile};

        duplicate_test_file(|src_filename| {
            let f = FitsFile::open(src_filename).unwrap();

            // open file manually
            let mut fptr: *mut fitsfile = std::ptr::null_mut();
            let mut status = 0;
            let c_filename = CString::new(src_filename).unwrap();
            unsafe {
                ffopen(
                    &mut fptr as *mut *mut fitsfile,
                    c_filename.as_ptr(),
                    FileOpenMode::READONLY as c_int,
                    &mut status,
                )
            };

            if status != 0 {
                panic!("error opening file manually");
            }

            assert!(!fptr.is_null());

            let f2 = unsafe { FitsFile::from_raw(fptr, FileOpenMode::READONLY).unwrap() };

            assert_eq!(f.file_path(), Path::new(src_filename));
            assert_eq!(f2.file_path(), Path::new(src_filename));
        });
    }

    #[cfg(feature = "rsfitsio-experimental")]
    #[test]
    fn test_get_filename_rsfitsio() {
        use rsfitsio::aliases::rust_api::fits_open_file;

        duplicate_test_file(|src_filename| {
            let mut fptr = None;
            let mut status = 0;
            let c_filename = CString::new(src_filename).unwrap();
            fits_open_file(
                &mut fptr,
                bytemuck::cast_slice(c_filename.as_bytes_with_nul()),
                FileOpenMode::READONLY as c_int,
                &mut status,
            );
            assert_eq!(status, 0, "error opening file manually");

            let mut f = FitsFile::from_rsfitsio(fptr.unwrap(), FileOpenMode::READONLY).unwrap();
            assert_eq!(f.file_path(), Path::new(src_filename));
            assert_eq!(f.num_hdus().unwrap(), 2);
        });
    }
}
