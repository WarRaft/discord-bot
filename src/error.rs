use std::panic::Location;
use std::{fmt, sync::Arc};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Cause {
    Any(BotError),
    Std(Arc<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct BotError {
    pub key: &'static str,
    pub causes: Vec<Cause>,
    pub file: &'static str,
    pub line: u32,
}

impl BotError {
    #[track_caller]
    #[inline]
    pub fn new(key: &'static str) -> Self {
        let loc = Location::caller();
        Self {
            key,
            causes: Vec::new(),
            file: loc.file(),
            line: loc.line(),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn new_at(key: &'static str, file: &'static str, line: u32) -> Self {
        Self {
            key,
            causes: Vec::new(),
            file,
            line,
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn push_any(mut self, cause: BotError) -> Self {
        self.causes.push(Cause::Any(cause));
        self
    }
    
    #[inline]
    #[allow(dead_code)]
    pub fn push_str(mut self, message: String) -> Self {
        self.causes.push(Cause::Std(Arc::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            message,
        ))));
        self
    }

    #[inline]
    pub fn push_std(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.causes.push(Cause::Std(Arc::new(cause)));
        self
    }

    /// Print error with ASCII tree of causes
    pub fn print_tree(&self) {
        eprintln!("[ERROR] {}:{} - {}", self.file, self.line, self.key);
        self.print_causes("", true);
    }

    fn print_causes(&self, prefix: &str, _is_last: bool) {
        for (i, cause) in self.causes.iter().enumerate() {
            let is_last_cause = i == self.causes.len() - 1;
            let branch = if is_last_cause { "└── " } else { "├── " };
            let extension = if is_last_cause { "    " } else { "│   " };
            
            match cause {
                Cause::Any(e) => {
                    eprintln!("{}{}[{}:{}] {}", prefix, branch, e.file, e.line, e.key);
                    e.print_causes(&format!("{}{}", prefix, extension), is_last_cause);
                }
                Cause::Std(e) => {
                    eprintln!("{}{}{}", prefix, branch, e);
                    
                    // Print nested sources
                    let mut source = e.source();
                    let mut depth = 0;
                    while let Some(err) = source {
                        let sub_branch = "    ↳ ";
                        eprintln!("{}{}{}{}", prefix, extension, "  ".repeat(depth), sub_branch);
                        eprintln!("{}{}{}  {}", prefix, extension, "  ".repeat(depth + 1), err);
                        source = err.source();
                        depth += 1;
                    }
                }
            }
        }
    }
}

impl fmt::Display for BotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}:{}] {}", self.file, self.line, self.key)?;
        if !self.causes.is_empty() {
            write!(f, " (causes: {})", self.causes.len())?;
        }
        Ok(())
    }
}

impl std::error::Error for BotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.causes.iter().find_map(|c| match c {
            Cause::Any(e) => Some(e as &dyn std::error::Error),
            Cause::Std(e) => Some(e.as_ref()),
        })
    }
}

// Implementations for Discord bot specific errors
impl From<reqwest::Error> for BotError {
    #[track_caller]
    fn from(e: reqwest::Error) -> Self {
        BotError::new("reqwest").push_std(e)
    }
}

impl From<serde_json::Error> for BotError {
    #[track_caller]
    fn from(e: serde_json::Error) -> Self {
        BotError::new("serde_json").push_std(e)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for BotError {
    #[track_caller]
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        BotError::new("websocket").push_std(e)
    }
}

impl From<url::ParseError> for BotError {
    #[track_caller]
    fn from(e: url::ParseError) -> Self {
        BotError::new("url_parse").push_std(e)
    }
}

impl From<std::env::VarError> for BotError {
    #[track_caller]
    fn from(e: std::env::VarError) -> Self {
        BotError::new("env_var").push_std(e)
    }
}

impl From<std::io::Error> for BotError {
    #[track_caller]
    fn from(e: std::io::Error) -> Self {
        BotError::new("io_error").push_std(e)
    }
}

impl From<mongodb::error::Error> for BotError {
    #[track_caller]
    fn from(e: mongodb::error::Error) -> Self {
        BotError::new("mongodb_error").push_std(e)
    }
}

impl From<String> for BotError {
    #[track_caller]
    fn from(s: String) -> Self {
        BotError::new("string_error").push_std(std::io::Error::new(std::io::ErrorKind::Other, s))
    }
}

impl From<&str> for BotError {
    #[track_caller]
    fn from(s: &str) -> Self {
        BotError::new("str_error").push_std(std::io::Error::new(std::io::ErrorKind::Other, s))
    }
}

pub type Result<T> = std::result::Result<T, BotError>;
