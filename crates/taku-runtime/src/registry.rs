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
