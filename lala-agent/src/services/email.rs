// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use anyhow::{Context, Result};
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use std::env;

/// Configuration for the email service.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub smtp_tls: bool,
    pub from_email: String,
    pub from_name: String,
    pub app_base_url: String,
    pub magic_link_expiry_minutes: u64,
    pub invitation_expiry_days: u64,
}

impl EmailConfig {
    /// Load email configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            smtp_host: env::var("SMTP_HOST").context("SMTP_HOST must be set")?,
            smtp_port: env::var("SMTP_PORT")
                .unwrap_or_else(|_| "587".to_string())
                .parse()
                .context("SMTP_PORT must be a valid port number")?,
            smtp_username: env::var("SMTP_USERNAME").context("SMTP_USERNAME must be set")?,
            smtp_password: env::var("SMTP_PASSWORD").context("SMTP_PASSWORD must be set")?,
            smtp_tls: env::var("SMTP_TLS").map(|v| v == "true").unwrap_or(true),
            from_email: env::var("SMTP_FROM_EMAIL").context("SMTP_FROM_EMAIL must be set")?,
            from_name: env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "LalaSearch".to_string()),
            app_base_url: env::var("APP_BASE_URL").context("APP_BASE_URL must be set")?,
            magic_link_expiry_minutes: env::var("MAGIC_LINK_EXPIRY_MINUTES")
                .unwrap_or_else(|_| "15".to_string())
                .parse()
                .context("MAGIC_LINK_EXPIRY_MINUTES must be a valid number")?,
            invitation_expiry_days: env::var("INVITATION_EXPIRY_DAYS")
                .unwrap_or_else(|_| "7".to_string())
                .parse()
                .context("INVITATION_EXPIRY_DAYS must be a valid number")?,
        })
    }
}

/// Email template with simple variable substitution.
struct EmailTemplate {
    content: &'static str,
}

impl EmailTemplate {
    const fn new(content: &'static str) -> Self {
        Self { content }
    }

    fn render(&self, vars: &[(&str, &str)]) -> String {
        let mut result = self.content.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }
}

// Email templates loaded at compile time
const MAGIC_LINK_TEMPLATE: EmailTemplate =
    EmailTemplate::new(include_str!("../../templates/emails/magic_link.txt"));
const INVITATION_TEMPLATE: EmailTemplate =
    EmailTemplate::new(include_str!("../../templates/emails/invitation.txt"));

/// Email service for sending authentication emails.
pub struct EmailService {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from_mailbox: Mailbox,
    config: EmailConfig,
}

impl EmailService {
    /// Create a new email service with the given configuration.
    pub fn new(config: EmailConfig) -> Result<Self> {
        let creds = Credentials::new(config.smtp_username.clone(), config.smtp_password.clone());

        let transport = if config.smtp_tls {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
                .context("Failed to create SMTP relay")?
                .port(config.smtp_port)
                .credentials(creds)
                .build()
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
                .port(config.smtp_port)
                .credentials(creds)
                .build()
        };

        let from_mailbox: Mailbox = format!("{} <{}>", config.from_name, config.from_email)
            .parse()
            .context("Invalid from email address")?;

        Ok(Self {
            transport,
            from_mailbox,
            config,
        })
    }

    /// Send a magic link email for authentication.
    pub async fn send_magic_link(&self, to_email: &str, token: &str) -> Result<()> {
        let verify_link = format!("{}/auth/verify/{}", self.config.app_base_url, token);
        let expiry_minutes = self.config.magic_link_expiry_minutes.to_string();

        let body = MAGIC_LINK_TEMPLATE.render(&[
            ("verify_link", &verify_link),
            ("expiry_minutes", &expiry_minutes),
        ]);

        self.send_email(to_email, "Sign in to LalaSearch", &body)
            .await
    }

    /// Send an organization invitation email.
    pub async fn send_invitation(
        &self,
        to_email: &str,
        org_name: &str,
        inviter_email: &str,
        token: &str,
    ) -> Result<()> {
        let invite_link = format!(
            "{}/auth/invitations/{}/accept",
            self.config.app_base_url, token
        );
        let expiry_days = self.config.invitation_expiry_days.to_string();

        let body = INVITATION_TEMPLATE.render(&[
            ("org_name", org_name),
            ("inviter_email", inviter_email),
            ("invite_link", &invite_link),
            ("expiry_days", &expiry_days),
        ]);

        self.send_email(to_email, &format!("Join {} on LalaSearch", org_name), &body)
            .await
    }

    /// Send an email.
    async fn send_email(&self, to: &str, subject: &str, body: &str) -> Result<()> {
        let to_mailbox: Mailbox = to.parse().context("Invalid recipient email address")?;

        let email = Message::builder()
            .from(self.from_mailbox.clone())
            .to(to_mailbox)
            .subject(subject)
            .body(body.to_string())
            .context("Failed to build email message")?;

        self.transport
            .send(email)
            .await
            .context("Failed to send email")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_render() {
        let template = EmailTemplate::new("Hello {{name}}, your code is {{code}}.");
        let result = template.render(&[("name", "Alice"), ("code", "12345")]);
        assert_eq!(result, "Hello Alice, your code is 12345.");
    }

    #[test]
    fn test_template_render_missing_var() {
        let template = EmailTemplate::new("Hello {{name}}, welcome!");
        let result = template.render(&[]);
        assert_eq!(result, "Hello {{name}}, welcome!");
    }

    #[test]
    fn test_magic_link_template_loads() {
        let result = MAGIC_LINK_TEMPLATE.render(&[
            ("verify_link", "https://example.com/verify/abc123"),
            ("expiry_minutes", "15"),
        ]);
        assert!(result.contains("https://example.com/verify/abc123"));
        assert!(result.contains("15 minutes"));
    }

    #[test]
    fn test_invitation_template_loads() {
        let result = INVITATION_TEMPLATE.render(&[
            ("org_name", "Acme Corp"),
            ("inviter_email", "admin@acme.com"),
            ("invite_link", "https://example.com/invite/xyz"),
            ("expiry_days", "7"),
        ]);
        assert!(result.contains("Acme Corp"));
        assert!(result.contains("admin@acme.com"));
        assert!(result.contains("https://example.com/invite/xyz"));
        assert!(result.contains("7 days"));
    }
}
