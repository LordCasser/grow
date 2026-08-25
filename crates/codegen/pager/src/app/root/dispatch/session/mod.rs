//! Session lifecycle, loading, picking, modal, and fork dispatchers.

pub(in crate::app::root::dispatch) mod fork;
pub(in crate::app::root::dispatch) mod lifecycle;
pub(in crate::app::root::dispatch) mod list;
pub(in crate::app::root::dispatch) mod load;
pub(in crate::app::root::dispatch) mod modal;
