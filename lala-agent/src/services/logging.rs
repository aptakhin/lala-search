// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Logging utilities for sensitive data anonymization.

/// Anonymize an email address for logging.
/// Shows first character and domain, hides the rest: "a***@example.com"
pub fn anonymize_email(email: &str) -> String {
    if let Some((local, domain)) = email.split_once('@') {
        if local.is_empty() {
            return format!("***@{}", domain);
        }
        let first = local.chars().next().unwrap_or('*');
        format!("{}***@{}", first, domain)
    } else {
        // Invalid email format, redact completely
        "***@***".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anonymize_email() {
        assert_eq!(anonymize_email("alice@example.com"), "a***@example.com");
        assert_eq!(anonymize_email("bob@test.org"), "b***@test.org");
    }

    #[test]
    fn test_anonymize_email_single_char() {
        assert_eq!(anonymize_email("x@example.com"), "x***@example.com");
    }

    #[test]
    fn test_anonymize_email_empty_local() {
        assert_eq!(anonymize_email("@example.com"), "***@example.com");
    }

    #[test]
    fn test_anonymize_email_no_at() {
        assert_eq!(anonymize_email("notanemail"), "***@***");
    }
}
