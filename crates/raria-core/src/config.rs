#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RariaConfig {
    pub rpc_listen_port: u16,
    pub rpc_listen_all: bool,
    pub control_file_extension: &'static str,
}

impl Default for RariaConfig {
    fn default() -> Self {
        Self {
            rpc_listen_port: 6800,
            rpc_listen_all: false,
            control_file_extension: ".raria",
        }
    }
}
