//! Shared support for campaign probe binaries. One binary per live probe,
//! because one process gets exactly one replay session (HANDOFF section 4).

pub mod guard;
pub mod reply;
pub mod session;
