#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    SymTool(symtool_backend::error::Error),
    Regex(regex::Error),
    Message(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{}", e),
            Self::SymTool(e) => write!(f, "{}", e),
            Self::Regex(e) => write!(f, "{}", e),
            Self::Message(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::SymTool(e) => Some(e),
            Self::Regex(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<symtool_backend::error::Error> for Error {
    fn from(err: symtool_backend::error::Error) -> Self {
        Self::SymTool(err)
    }
}

impl From<regex::Error> for Error {
    fn from(err: regex::Error) -> Self {
        Self::Regex(err)
    }
}
