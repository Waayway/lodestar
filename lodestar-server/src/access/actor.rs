/// Who performed a request; inserted as a request extension for handlers.
#[derive(Clone, Debug, PartialEq)]
pub struct Actor(pub String);
