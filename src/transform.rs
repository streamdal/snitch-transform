use streamdal_gjson as gjson;

#[derive(Debug)]
pub enum TransformError {
    Generic(String),
}

pub struct Request {
    pub data: Vec<u8>,
    pub path: String,
    pub value: String,
}

pub fn overwrite(req: &Request) -> Result<String, TransformError> {
    validate_request(req, true)?;

    let data = gjson::set_overwrite(
        convert_bytes_to_string(&req.data)?,
        req.path.as_str(),
        req.value.as_str(),
    )
    .map_err(|e| TransformError::Generic(format!("unable to overwrite data: {}", e)))?;

    Ok(data)
}

pub fn obfuscate(req: &Request) -> Result<String, TransformError> {
    validate_request(req, false)?;

    let data_as_str = convert_bytes_to_string(&req.data)?;
    let value = gjson::get(data_as_str, req.path.as_str());

    match value.kind() {
        gjson::Kind::String => _obfuscate(data_as_str, req.path.as_str()),
        _ => Err(TransformError::Generic(format!(
            "unable to mask data: path '{}' is not a string or number",
            req.path
        ))),
    }
}

fn _obfuscate(data: &str, path: &str) -> Result<String, TransformError> {
    let contents = gjson::get(data, path);
    let hashed = sha256::digest(contents.str().as_bytes());

    let obfuscated = format!("\"sha256:{}\"", hashed);

    gjson::set_overwrite(data, path, &obfuscated)
        .map_err(|e| TransformError::Generic(format!("unable to obfuscate data: {}", e)))
}

pub fn mask(req: &Request) -> Result<String, TransformError> {
    validate_request(req, false)?;

    let data_as_str = convert_bytes_to_string(&req.data)?;
    let value = gjson::get(data_as_str, req.path.as_str());

    match value.kind() {
        gjson::Kind::String => _mask(data_as_str, req.path.as_str(), '*', true),
        gjson::Kind::Number => _mask(data_as_str, req.path.as_str(), '0', false),
        _ => Err(TransformError::Generic(format!(
            "unable to mask data: path '{}' is not a string or number",
            req.path
        ))),
    }
}

fn _mask(data: &str, path: &str, mask_char: char, quote: bool) -> Result<String, TransformError> {
    let contents = gjson::get(data, path);
    let num_chars_to_mask = (0.8 * contents.str().len() as f64).round() as usize;
    let num_chars_to_skip = contents.str().len() - num_chars_to_mask;

    let mut masked = contents.str()[0..num_chars_to_skip].to_string()
        + mask_char.to_string().repeat(num_chars_to_mask).as_str();

    if quote {
        masked = format!("\"{}\"", masked);
    }

    gjson::set_overwrite(data, path, &masked)
        .map_err(|e| TransformError::Generic(format!("unable to mask data: {}", e)))
}

fn validate_request(req: &Request, value_check: bool) -> Result<(), TransformError> {
    if req.path.is_empty() {
        return Err(TransformError::Generic("path cannot be empty".to_string()));
    }

    if req.data.is_empty() {
        return Err(TransformError::Generic("data cannot be empty".to_string()));
    }

    if value_check && req.value.is_empty() {
        return Err(TransformError::Generic("value cannot be empty".to_string()));
    }

    // Is this valid JSON?
    if !gjson::valid(convert_bytes_to_string(&req.data)?) {
        return Err(TransformError::Generic(
            format!("data is not valid JSON: {}", String::from_utf8(req.data.to_vec()).unwrap()),
        ));
    }

    // Valid path?
    if !gjson::get(convert_bytes_to_string(&req.data)?, req.path.as_str()).exists() {
        return Err(TransformError::Generic(format!(
            "path '{}' not found in data",
            req.path
        )));
    }

    Ok(())
}

fn convert_bytes_to_string(bytes: &Vec<u8>) -> Result<&str, TransformError> {
    Ok(std::str::from_utf8(bytes.as_slice())
        .map_err(|e| TransformError::Generic(format!("unable to parse data as UTF-8: {}", e))))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DATA: &str = r#"{
    "foo": "bar",
    "baz": {
        "qux": "quux"
    },
    "recipient": "tledwichb9@wsj.com",
    "bool": true
}"#;

    #[test]
    fn test_overwrite() {
        let mut req = Request {
            data: TEST_DATA.as_bytes().to_vec(),
            path: "baz.qux".to_string(),
            value: "\"test\"".to_string(),
        };

        let result = overwrite(&req).unwrap();

        assert!(gjson::valid(&TEST_DATA));
        assert!(gjson::valid(&result));
        assert_eq!(result, TEST_DATA.replace("quux", "test"));

        let v = gjson::get(TEST_DATA, "baz.qux");
        assert_eq!(v.str(), "quux");

        let v = gjson::get(result.as_str(), "baz.qux");
        assert_eq!(v.str(), "test");

        req.path = "does-not-exist".to_string();
        assert!(
            overwrite(&req).is_err(),
            "should error when path does not exist"
        );

        // Can overwrite anything
        req.path = "bool".to_string();
        assert!(
            overwrite(&req).is_ok(),
            "should be able to replace any value, regardless of type"
        );
    }

    #[test]
    fn test_obfuscate() {
        let mut req = Request {
            data: TEST_DATA.as_bytes().to_vec(),
            path: "baz.qux".to_string(),
            value: "".to_string(), // needs a default
        };

        let result = obfuscate(&req).unwrap();
        let hashed_value = sha256::digest("quux".as_bytes());

        assert!(gjson::valid(&TEST_DATA));
        assert!(gjson::valid(&result));

        let v = gjson::get(TEST_DATA, "baz.qux");
        assert_eq!(v.str(), "quux");

        let v = gjson::get(result.as_str(), "baz.qux");
        assert_eq!(v.str(), format!("sha256:{}", hashed_value));

        // path does not exist
        req.path = "does-not-exist".to_string();
        assert!(mask(&req).is_err());

        // path not a string
        req.path = "bool".to_string();
        assert!(mask(&req).is_err());
    }

    #[test]
    fn test_mask_email() {
        for _ in 0..1000000 {
            let req = Request {
                data: TEST_DATA.as_bytes().to_vec(),
                path: "recipient".to_string(),
                value: "".to_string(), // needs a default
            };

            let result = mask(&req).unwrap();

            assert!(gjson::valid(TEST_DATA));
            assert!(gjson::valid(&result));

            let v = gjson::get(TEST_DATA, "recipient");
            assert_eq!(v.str(), "tledwichb9@wsj.com");

            let v2 = gjson::get(result.as_str(), "recipient");
            assert_ne!(v2.str(), "tledwichb9@wsj.com");
            assert_eq!(v2.str(), "tled**************");
        }
    }

    #[test]
    fn test_mask() {
        let mut req = Request {
            data: TEST_DATA.as_bytes().to_vec(),
            path: "baz.qux".to_string(),
            value: "".to_string(), // needs a default
        };

        let result = mask(&req).unwrap();

        assert!(gjson::valid(TEST_DATA));
        assert!(gjson::valid(&result));

        let v = gjson::get(TEST_DATA, "baz.qux");
        assert_eq!(v.str(), "quux");

        let v = gjson::get(result.as_str(), "baz.qux");
        assert_eq!(v.str(), "q***");

        // path does not exist
        req.path = "does-not-exist".to_string();
        assert!(mask(&req).is_err());

        // path not a string
        req.path = "bool".to_string();
        assert!(mask(&req).is_err());
    }
}
