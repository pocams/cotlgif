use headers::{Header, HeaderName, HeaderValue};
use once_cell::sync::OnceCell;

// https://docs.rs/headers/0.3.8/headers/index.html#implementing-the-header-trait

static X_REAL_IP: OnceCell<HeaderName> = OnceCell::new();

#[derive(Debug)]
pub struct XRealIp(pub String);

impl Header for XRealIp {
    fn name() -> &'static HeaderName {
        X_REAL_IP.get_or_init(|| HeaderName::from_static("x-real-ip"))
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;

        let s = value.to_str().ok().ok_or_else(headers::Error::invalid)?;

        Ok(XRealIp(s.to_string()))
    }

    fn encode<E>(&self, values: &mut E)
    where
        E: Extend<HeaderValue>,
    {
        values.extend(std::iter::once(HeaderValue::from_str(&self.0).unwrap()));
    }
}
