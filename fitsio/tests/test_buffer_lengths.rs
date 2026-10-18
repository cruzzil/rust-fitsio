//! Inputs that would make CFITSIO read or write past the end of a buffer are rejected or sized
//! for, on both backends.

use fitsio::images::{ImageDescription, ImageType};
use fitsio::tables::{ColumnDataType, ColumnDescription};
use fitsio::FitsFile;
use tempfile::TempDir;

fn temp_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("test.fits")
}

#[test]
fn string_column_narrower_display_width() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = temp_path(&dir);
    let data: Vec<String> = (0..5).map(|i| format!("{:0>20}", i)).collect();
    {
        let mut f = FitsFile::create(&path).open()?;
        let columns = [ColumnDescription::new("NAME")
            .with_type(ColumnDataType::String)
            .that_repeats(20)
            .create()?];
        let hdu = f.create_table("TABLE".to_string(), &columns)?;
        hdu.write_col(&mut f, "NAME", &data)?;
        // displayed in 5 characters, but stored in 20
        hdu.write_key(&mut f, "TDISP1", "A5")?;
    }

    let mut f = FitsFile::open(&path)?;
    let hdu = f.hdu("TABLE")?;
    let read: Vec<String> = hdu.read_col(&mut f, "NAME")?;
    assert_eq!(read, data);
    Ok(())
}

#[test]
fn logical_column_as_strings() -> Result<(), Box<dyn std::error::Error>> {
    let mut f = FitsFile::open("../testdata/boolean_columns.fits")?;
    let hdu = f.hdu(1)?;
    let bools: Vec<bool> = hdu.read_col(&mut f, "Whitening_Filter")?;
    let strings: Vec<String> = hdu.read_col(&mut f, "Whitening_Filter")?;
    let expected: Vec<&str> = bools.iter().map(|b| if *b { "T" } else { "F" }).collect();
    assert_eq!(strings, expected);
    Ok(())
}

#[test]
fn write_col_range_with_too_little_data() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut f = FitsFile::create(temp_path(&dir)).open()?;
    let columns = [ColumnDescription::new("A")
        .with_type(ColumnDataType::Int)
        .create()?];
    let hdu = f.create_table("TABLE".to_string(), &columns)?;
    assert!(hdu
        .write_col_range(&mut f, "A", &[1i32, 2], &(0..100))
        .is_err());
    Ok(())
}

fn image(dir: &TempDir) -> Result<(FitsFile, fitsio::hdu::FitsHdu), Box<dyn std::error::Error>> {
    let mut f = FitsFile::create(temp_path(dir)).open()?;
    let desc = ImageDescription {
        data_type: ImageType::Long,
        dimensions: &[10, 10],
    };
    let hdu = f.create_image("IMAGE".to_string(), &desc)?;
    Ok((f, hdu))
}

#[test]
fn region_with_too_few_ranges() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let (mut f, hdu) = image(&dir)?;
    assert!(hdu.read_region::<Vec<i32>>(&mut f, &[&(0..2)]).is_err());
    assert!(hdu.write_region(&mut f, &[&(0..5)], &[1i32; 5]).is_err());
    Ok(())
}

#[test]
fn region_with_an_empty_range() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let (mut f, hdu) = image(&dir)?;
    assert!(hdu
        .read_region::<Vec<i32>>(&mut f, &[&(0..2), &(3..3)])
        .is_err());
    Ok(())
}

#[test]
fn write_region_with_too_little_data() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let (mut f, hdu) = image(&dir)?;
    assert!(hdu
        .write_region(&mut f, &[&(0..5), &(0..5)], &[1i32, 2, 3])
        .is_err());

    // the full region still round trips
    let data: Vec<i32> = (0..25).collect();
    hdu.write_region(&mut f, &[&(0..5), &(0..5)], &data)?;
    let read: Vec<i32> = hdu.read_region(&mut f, &[&(0..5), &(0..5)])?;
    assert_eq!(read, data);
    Ok(())
}

/// A table with an integer column `A` holding 1, 2, 3, and the given extra header keywords
fn int_table(
    dir: &TempDir,
    keys: &[(&str, &str)],
) -> Result<(FitsFile, fitsio::hdu::FitsHdu), Box<dyn std::error::Error>> {
    let mut f = FitsFile::create(temp_path(dir)).open()?;
    let columns = [ColumnDescription::new("A")
        .with_type(ColumnDataType::Int)
        .create()?];
    let hdu = f.create_table("TABLE".to_string(), &columns)?;
    hdu.write_col(&mut f, "A", &[1i32, 2, 3])?;
    for (key, value) in keys {
        hdu.write_key(&mut f, key, *value)?;
    }
    Ok((f, hdu))
}

#[test]
fn integer_column_with_nulls_as_strings() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut f = FitsFile::create(temp_path(&dir)).open()?;
    let columns = [ColumnDescription::new("A")
        .with_type(ColumnDataType::Int)
        .create()?];
    let hdu = f.create_table("TABLE".to_string(), &columns)?;
    hdu.write_key(&mut f, "TNULL1", -99i64)?;
    let hdu = f.hdu("TABLE")?;
    hdu.write_col(&mut f, "A", &[1i32, -99, 3])?;

    let read: Vec<String> = hdu.read_col(&mut f, "A")?;
    assert_eq!(read.len(), 3);
    assert_eq!(read[0].trim(), "1");
    assert_eq!(read[2].trim(), "3");
    Ok(())
}

#[test]
fn display_formats_cfitsio_cannot_hold() -> Result<(), Box<dyn std::error::Error>> {
    // too long for CFITSIO's 20 character buffers, too wide for an int, and too wide to format
    for tdisp in ["I1234567890123456789", "I3000000000", "I200000000"] {
        let dir = tempfile::tempdir()?;
        let (mut f, hdu) = int_table(&dir, &[("TDISP1", tdisp)])?;
        assert!(
            hdu.read_col::<String>(&mut f, "A").is_err(),
            "TDISP1 = {}",
            tdisp
        );
        // reading the values themselves does not use the display format
        assert_eq!(hdu.read_col::<i32>(&mut f, "A")?, [1, 2, 3]);
    }
    Ok(())
}
