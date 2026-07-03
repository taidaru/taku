use taku_api::ApiEntry;

/// The single list of every API taku registers. Adding an API touches this
/// list plus Cargo.toml
pub(crate) fn apis() -> Vec<ApiEntry> {
    vec![
        taku_fs::API,
        taku_shell::API,
        taku_env::API,
        taku_net::API,
        taku_ssh::API,
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// `ssh` is the one API that can't be rerouted over itself; every other
    /// registry entry needs a `REMOTE_APIS` row in taku-ssh, or `ssh.on`
    /// silently keeps the local backend for that global.
    #[test]
    fn remote_apis_track_the_registry() {
        let local: BTreeSet<&str> = super::apis()
            .iter()
            .map(|api| api.global)
            .filter(|global| *global != "ssh")
            .collect();
        let remote: BTreeSet<&str> = taku_ssh::remote_globals().collect();
        assert_eq!(
            local, remote,
            "registry::apis() and taku-ssh's REMOTE_APIS drifted apart; \
             new APIs must be added to both (or excluded here if not remotable)"
        );
    }
}
