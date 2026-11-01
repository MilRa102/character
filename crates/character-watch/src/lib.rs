//! System event observers (MVP-1), built on `zbus`.
//!
//! Sources: notifications, MPRIS, UPower, NetworkManager, logind, udisks2/bluez,
//! systemd/journald and the Wayland toplevel/idle protocols. Only metadata is
//! observed: screen and input contents are never read.
