use std::error::Error as StdError;
use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Error {
    inner: Box<dyn StdError + Send + Sync + 'static>,
    contexts: Vec<String>,
}

impl Error {
    pub fn new<E>(error: E) -> Self
    where E: StdError + Send + Sync + 'static {
        Error { inner: Box::new(error), contexts: Vec::new() }
    }

    pub fn msg<M>(message: M) -> Self
    where M: fmt::Display + fmt::Debug + Send + Sync + 'static {
        Error { inner: Box::new(MessageError(message.to_string())), contexts: Vec::new() }
    }

    pub fn context<C>(mut self, context: C) -> Self
    where C: fmt::Display + Send + Sync + 'static {
        self.contexts.push(context.to_string());
        self
    }

    pub fn downcast_ref<E: StdError + 'static>(&self) -> Option<&E> {
        self.inner.downcast_ref::<E>()
    }

    pub fn chain(&self) -> Chain<'_> {
        Chain { ctx: self.contexts.iter().rev(), current: Some(&*self.inner) }
    }

    pub fn root_cause(&self) -> &(dyn StdError + 'static) {
        let mut current: &dyn StdError = &*self.inner;
        while let Some(source) = current.source() { current = source; }
        current
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut causes: Vec<String> = Vec::new();
        for context in self.contexts.iter().rev().skip(1) {
            causes.push(context.clone());
        }
        if !self.contexts.is_empty() { causes.push(self.inner.to_string()); }
        let mut source = self.inner.source();
        while let Some(error) = source {
            causes.push(error.to_string());
            source = error.source();
        }
        match self.contexts.last() {
            Some(top) => write!(f, "{}", top)?,
            None => write!(f, "{}", self.inner)?,
        }
        if !causes.is_empty() {
            write!(f, "\n\nCaused by:")?;
            for (idx, cause) in causes.iter().enumerate() {
                write!(f, "\n    {}: {}", idx, cause)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.contexts.last() {
            Some(top) => write!(f, "{}", top),
            None => write!(f, "{}", self.inner),
        }
    }
}

impl<E: StdError + Send + Sync + 'static> From<E> for Error {
    fn from(error: E) -> Self { Error::new(error) }
}

pub struct Chain<'a> {
    ctx: std::iter::Rev<std::slice::Iter<'a, String>>,
    current: Option<&'a (dyn StdError + 'static)>,
}

impl<'a> Iterator for Chain<'a> {
    type Item = ChainItem<'a>;
    fn next(&mut self) -> Option<ChainItem<'a>> {
        if let Some(context) = self.ctx.next() {
            return Some(ChainItem::Context(context));
        }
        let current = self.current?;
        self.current = current.source();
        Some(ChainItem::Error(current))
    }
}

pub enum ChainItem<'a> {
    Context(&'a str),
    Error(&'a (dyn StdError + 'static)),
}

impl<'a> fmt::Display for ChainItem<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChainItem::Context(s) => f.write_str(s),
            ChainItem::Error(e) => write!(f, "{}", e),
        }
    }
}

#[derive(Debug)]
struct MessageError(String);

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.0) }
}

impl StdError for MessageError {}

pub trait Context<T, E> {
    fn context<C>(self, context: C) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static;
    fn with_context<C, F>(self, f: F) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static, F: FnOnce() -> C;
}

impl<T, E> Context<T, E> for std::result::Result<T, E>
where E: StdError + Send + Sync + 'static
{
    fn context<C>(self, context: C) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static {
        match self {
            Ok(v) => Ok(v),
            Err(error) => Err(Error::new(error).context(context)),
        }
    }
    fn with_context<C, F>(self, f: F) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static, F: FnOnce() -> C {
        match self {
            Ok(v) => Ok(v),
            Err(error) => Err(Error::new(error).context(f())),
        }
    }
}

impl<T> Context<T, Error> for Result<T> {
    fn context<C>(self, context: C) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static {
        match self {
            Ok(v) => Ok(v),
            Err(error) => Err(error.context(context)),
        }
    }
    fn with_context<C, F>(self, f: F) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static, F: FnOnce() -> C {
        match self {
            Ok(v) => Ok(v),
            Err(error) => Err(error.context(f())),
        }
    }
}

impl<T> Context<T, std::convert::Infallible> for std::option::Option<T> {
    fn context<C>(self, context: C) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static {
        match self {
            Some(v) => Ok(v),
            None => Err(Error::msg(context.to_string())),
        }
    }
    fn with_context<C, F>(self, f: F) -> Result<T>
    where C: fmt::Display + Send + Sync + 'static, F: FnOnce() -> C {
        match self {
            Some(v) => Ok(v),
            None => Err(Error::msg(f().to_string())),
        }
    }
}

#[macro_export]
macro_rules! anyhow {
    ($msg:literal $(,)?) => { $crate::anyhow::Error::msg(format!($msg)) };
    ($err:expr $(,)?) => { $crate::anyhow::Error::msg($err.to_string()) };
    ($fmt:expr, $($arg:tt)*) => { $crate::anyhow::Error::msg(format!($fmt, $($arg)*)) };
}

#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => { return Err($crate::anyhow!($($arg)*)) };
}

#[macro_export]
macro_rules! ensure {
    ($cond:expr, $($arg:tt)*) => {
        if !$cond { return Err($crate::anyhow!($($arg)*)); }
    };
}
