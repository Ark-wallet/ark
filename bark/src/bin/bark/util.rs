
use std::io::{self, Write};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use serde_json;
use tonic::transport::Uri;


/// Wrap a [Duration] so that it implements [fmt::Display] to show a
/// human-readable duration.
///
/// Will show:
/// - seconds until 60 sec
/// - minutes until 60 min
/// - hours until 48 hours
/// - days
#[derive(Clone)]
pub struct PrettyDuration(pub Duration);

impl fmt::Display for PrettyDuration {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let dur = self.0;
		if dur < Duration::from_secs(60) {
			write!(f, "{} seconds", dur.as_secs())
		} else if dur < Duration::from_secs(60 * 60) {
			write!(f, "{} minutes", dur.as_secs().div_ceil(60))
		} else if dur < Duration::from_secs(48 * 60 * 60) {
			write!(f, "{:.2} hours", dur.as_secs() as f64 / (60 * 60) as f64)
		} else {
			write!(f, "{:.2} days", dur.as_secs() as f64 / (24 * 60 * 60) as f64)
		}
	}
}

/// Parse the URL and add `https` scheme if no scheme is given.
pub fn https_default_scheme(url: String) -> anyhow::Result<String> {
	// default scheme to https if unset
	let mut uri_parts = Uri::from_str(&url).context("invalid url")?.into_parts();
	if uri_parts.authority.is_none() {
		bail!("invalid url '{}': missing authority", url);
	}
	if uri_parts.scheme.is_none() {
		uri_parts.scheme = Some("https".parse().unwrap());
		// because from_parts errors for missing PathAndQuery, set it
		uri_parts.path_and_query = Some(uri_parts.path_and_query
			.unwrap_or_else(|| "".parse().unwrap())
		);
		let new = Uri::from_parts(uri_parts).unwrap();
		Ok(new.to_string())
	} else {
		Ok(url)
	}
}

/// Writes a [`Serializable`] value to stdout
pub fn output_json<T>(value: &T) -> ()
where
	T: ?Sized + Serialize,
{
	serde_json::to_writer_pretty(io::stdout(), value).expect("value is serializable");
	write!(io::stdout(), "\n").expect("Failed to write newline to stdout");
}


#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn test_pretty_duration() {
		let secs = |s| PrettyDuration(Duration::from_secs(s)).to_string();
		assert_eq!("59 seconds", secs(59));
		assert_eq!("1 minutes", secs(60));
		assert_eq!("59 minutes", secs(58 * 60 + 1)); // ceil
		assert_eq!("1.00 hours", secs(60 * 60));
		assert_eq!("47.50 hours", secs(47 * 60 * 60 + 30 * 60));
		assert_eq!("2.00 days", secs(48 * 60 * 60));
	}
}
