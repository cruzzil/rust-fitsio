use crate::backend::{self, c_char, c_int, FLEN_STATUS};
use crate::errors::Result;

/// Helper function converting a nul-terminated C string buffer to a Rust String
///
/// Everything from the first nul onwards is ignored; a buffer without a nul is converted whole.
pub fn buf_to_string(buffer: &[c_char]) -> Result<String> {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    Ok(String::from_utf8(bytes)?)
}

/// Internal function to get the fits error description from a status code
pub fn status_to_string(status: c_int) -> Result<Option<String>> {
    match status {
        0 => Ok(None),
        status => {
            let mut buffer = [0; FLEN_STATUS];
            backend::fits_get_errstatus(status, &mut buffer);
            let result_str = buf_to_string(&buffer)?;
            Ok(Some(result_str))
        }
    }
}

#[cfg(test)]
mod test {
    use super::{buf_to_string, status_to_string};

    #[test]
    fn test_returning_error_messages() {
        assert_eq!(
            status_to_string(105).unwrap().unwrap(),
            "couldn't create the named file"
        );
    }

    #[test]
    fn test_buf_to_string_stops_at_nul() {
        let buf = [b'a' as _, b'b' as _, 0, b'c' as _];
        assert_eq!(buf_to_string(&buf).unwrap(), "ab");
        assert_eq!(buf_to_string(&buf[..2]).unwrap(), "ab");
    }
}
