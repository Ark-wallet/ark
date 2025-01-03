
#[macro_use] extern crate anyhow;
#[macro_use] extern crate log;

use std::{fs, process};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use aspd_log::RecordSerializeWrapper;
use bitcoin::{Address, Amount, FeeRate, Network};
use clap::Parser;
use tonic::transport::Uri;

use aspd::{App, Config, ClnConfig};
use aspd_rpc_client as rpc;

/// Defaults to our default port on localhost.
const DEFAULT_ADMIN_RPC_ADDR: &str = "127.0.0.1:3536";

#[derive(Parser)]
#[command(author = "Steven Roose <steven@roose.io>", version, about)]
struct Cli {
	/// The data directory for aspd, mandatory field for most commands
	#[arg(long, global = true, env = "ASPD_DATADIR")]
	datadir: Option<PathBuf>,
	#[command(subcommand)]
	command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
	/// Create and configure asp server
	#[command()]
	Create(CreateOpts),

	/// Set and update asp config
	#[command()]
	SetConfig(ConfigOpts),

	/// Start asp server
	#[command()]
	Start,

	/// Drain funds of asp
	#[command()]
	Drain {
		/// The address to send all the wallet funds to
		address: Address<bitcoin::address::NetworkUnchecked>,
	},

	/// Retrieve 12 word seed phrase
	#[command()]
	GetMnemonic,

	/// Drop out of round conflicts from database
	#[command()]
	DropOorConflicts,

	/// Run RPC commands
	#[command()]
	Rpc {
		#[arg(long, default_value = DEFAULT_ADMIN_RPC_ADDR)]
		addr: String,
		#[command(subcommand)]
		cmd: RpcCommand,
	},
}

#[derive(clap::Subcommand)]
enum RpcCommand {
	/// Get asp balance
	#[command()]
	Balance,

	/// Get asp address
	#[command()]
	GetAddress,

	/// Start a new asp round
	#[command()]
	TriggerRound,

	/// Stop aspd
	#[command()]
	Stop,
}

#[tokio::main]
async fn main() {
	if let Err(e) = inner_main().await {
		println!("An error occurred: {}", e);
		// maybe hide second print behind a verbose flag
		println!("");
		println!("{:?}", e);
		process::exit(1);
	}
}

fn init_logging() {
	//TODO(stevenroose) add filename and line number when verbose logging
	fern::Dispatch::new()
		.level(log::LevelFilter::Trace)
		.level_for("rustls", log::LevelFilter::Warn)
		.level_for("bitcoincore_rpc", log::LevelFilter::Warn)
		// regular logging dispatch
		.chain(fern::Dispatch::new()
			.format(|out, msg, rec| {
				let now = chrono::Local::now();
				let stamp = now.format("%Y-%m-%d %H:%M:%S.%3f");
				let kv = if rec.key_values().count() > 0 {
					let mut buf = Vec::new();
					buf.extend(" -- ".as_bytes());
					serde_json::to_writer(&mut buf, &aspd_log::SourceSerializeWrapper(rec.key_values())).unwrap();
					String::from_utf8(buf).unwrap()
				} else {
					String::new()
				};
				out.finish(format_args!(
					"[{} {: >5} {}] {}{}",
					stamp, rec.level(), rec.module_path().unwrap_or(""), msg, kv,
				))
			})
			.chain(std::io::stdout())
		)
		// structured logging dispatch
		.chain(fern::Dispatch::new()
			.filter(|m| m.target() == aspd_log::SLOG_TARGET)
			.format(|out, _msg, rec| {
				#[derive(serde::Serialize)]
				struct Rec<'a> {
					timestamp: chrono::DateTime<chrono::Local>,
					#[serde(flatten)]
					rec: RecordSerializeWrapper<'a>,
				}
				let rec = Rec {
					timestamp: chrono::Local::now(),
					rec: RecordSerializeWrapper(rec),
				};
				out.finish(format_args!("{}", serde_json::to_string(&rec).unwrap()));
			})
			.chain(std::io::stdout()) //TODO(stevenroose) pipe to file?
		)
		.apply().expect("error setting up logging");
}

async fn inner_main() -> anyhow::Result<()> {
	let cli = Cli::parse();

	if let Command::Rpc { cmd, addr } = cli.command {
		return run_rpc(&addr, cmd).await;
	}

	init_logging();

	match cli.command {
		Command::Rpc { .. } => unreachable!(),
		Command::Create(opts) => {
			let datadir = {
				let datadir = PathBuf::from(cli.datadir.context("need datadir")?);
				if !datadir.exists() {
					fs::create_dir_all(&datadir).context("failed to create datadir")?;
				}
				datadir.canonicalize().context("canonicalizing path")?
			};

			opts.config.validate()?;

			let mut cfg = Config {
				network: opts.network,
				..Default::default()
			};
			cfg = opts.config.merge(cfg)?;
			App::create(&datadir, cfg).await?;
		},
		Command::SetConfig(updates) => {
			let datadir = PathBuf::from(cli.datadir.context("need datadir")?);
			// Create a back-up of the old config file
			Config::create_backup_in_datadir(&datadir)?;

			// Update the configuration
			let cfg = Config::read_from_datadir(&datadir)?;
			let cfg = updates.merge(cfg)?;
			cfg.write_to_datadir(&datadir)?;

			println!("The configuration has been updated");
			println!("You should restart `arkd` to ensure the new configuration takes effect");
			println!("Current config: {:#?}", cfg);
		},
		Command::Start => {
			let mut app = App::open(&cli.datadir.context("need datadir")?).await.context("server init")?;
			if let Err(e) = app.start().await {
				error!("Shutdown error from aspd {:?}", e);
				process::exit(1);
			};
		},
		Command::Drain { address } => {
			let app = App::open(&cli.datadir.context("need datadir")?).await.context("server init")?;
			println!("{}", app.drain(address).await?.compute_txid());
		},
		Command::GetMnemonic => {
			let app = App::open(&cli.datadir.context("need datadir")?).await.context("server init")?;
			println!("{}", app.get_master_mnemonic()?);
		},
		Command::DropOorConflicts => {
			let app = App::open(&cli.datadir.context("need datadir")?).await.context("server init")?;
			app.drop_all_oor_conflicts()?;
		},
	}

	Ok(())
}

fn init_logging_rpc() {
	let colors = fern::colors::ColoredLevelConfig::default();
	fern::Dispatch::new()
		.level(log::LevelFilter::Trace)
		.level_for("rustls", log::LevelFilter::Warn)
		.level_for("bitcoincore_rpc", log::LevelFilter::Warn)
		.format(move |out, msg, rec| {
			let now = chrono::Local::now();
			// only time, not date
			let stamp = now.format("%H:%M:%S.%3f");
			out.finish(format_args!(
				"[{} {: >5}] {}", stamp, colors.color(rec.level()), msg,
			))
		})
		.chain(std::io::stderr())
		.apply().expect("error setting up logging");
}

async fn run_rpc(addr: &str, cmd: RpcCommand) -> anyhow::Result<()> {
	init_logging_rpc();

	let addr = if addr.starts_with("http") {
		addr.to_owned()
	} else {
		format!("http://{}", addr)
	};
	let asp_endpoint = Uri::from_str(&addr).context("invalid asp addr")?;
	let mut asp = rpc::AdminServiceClient::connect(asp_endpoint)
		.await.context("failed to connect to asp")?;

	match cmd {
		RpcCommand::Balance => {
			let res = asp.wallet_status(rpc::Empty {}).await?.into_inner();
			println!("{}", Amount::from_sat(res.balance));
		},
		RpcCommand::GetAddress => {
			let res = asp.wallet_status(rpc::Empty {}).await?.into_inner();
			println!("{}", res.address);
		},
		RpcCommand::TriggerRound => {
			asp.trigger_round(rpc::Empty {}).await?.into_inner();
		}
		RpcCommand::Stop => unimplemented!(),
	}
	Ok(())
}

#[derive(clap::Args)]
struct CreateOpts {
	#[arg(long, default_value = "regtest", env = "BITCOIND_NETWORK")]
	network: Network,


	#[command(flatten)]
	config: ConfigOpts,
}

#[derive(Debug, Clone, Default, clap::Args)]
struct ConfigOpts {
	/// the URL of the bitcoind RPC (mandatory on create)
	#[arg(long, env = "BITCOIND_URL")]
	bitcoind_url: Option<String>,

	/// the path of the cookie file for the bitcoind RPC
	/// It is mandatory to configure exactly one authentication method
	/// This could either be [bitcoind_cookie] or [bitcoind_rpc_user] and [bitcoind_rpc_pass]
	#[clap(long, env = "BITCOIND_COOKIE")]
	bitcoind_cookie: Option<PathBuf>,


	/// the user for the bitcoind RPC
	/// It is mandatory to configure exactly one authentication method
	/// If a [bitcoind_rpc_user] is provided [bitcoind_rpc_pass] must be provided
	#[clap(long, env = "BITCOIND_RPC_USER")]
	bitcoind_rpc_user: Option<String>,

	/// the password for the bitcoind RPC
	/// It is mandatory to configure exactly one authentication method
	/// If a [bitcoind_rpc_user] is provided [bitcoind_rpc_pass] must be provided
	#[clap(long, env = "BITCOIND_RPC_PASS")]
	bitcoind_rpc_pass: Option<String>,

	#[arg(long, env = "ASPD_PUBLIC_RPC_ADDRESS")]
	public_rpc_address: Option<String>,

	#[arg(long, env = "ASPD_ADMIN_RPC_ADDRESS")]
	admin_rpc_address: Option<Option<String>>,

	/// Round interval, in ms.
	#[arg(long, env = "ASPD_ROUND_INTERVAL")]
	round_interval: Option<u64>,
	/// Time for users to submit payments in rounds, in ms.
	#[arg(long, env = "ASPD_ROUND_SUBMIT_TIME")]
	round_submit_time: Option<u64>,
	/// Time for users to submit signatures in rounds, in ms.
	#[arg(long, env = "ASPD_ROUND_SIGN_TIME")]
	round_sign_time: Option<u64>,
	#[arg(long, env = "ASPD_NB_ROUND_NONCES")]
	nb_round_nonces: Option<usize>,
	#[arg(long, env = "ASPD_VTXO_EXPIRY_DELTA")]
	vtxo_expiry_delta: Option<u16>,
	#[arg(long, env = "ASPD_VTXO_EXIT_DELTA")]
	vtxo_exit_delta: Option<u16>,
	#[arg(long, env = "ASPD_SWEEP_THRESHOLD")]
	sweep_threshold: Option<Amount>,

	/// The feerate (in sats per kvb) to use for round txs.
	#[arg(long, env = "ASPD_ROUND_TX_FEERATE_SAT_PER_KVB")]
	round_tx_feerate_sat_per_kvb: Option<u64>,

	#[arg(long, env = "CLN_GRPC_URI")]
	cln_grpc_uri: Option<Option<Uri>>,
	#[arg(long, env = "CLN_GRPC_SERVER_CERT_PATH")]
	cln_grpc_server_cert_path: Option<Option<PathBuf>>,
	#[arg(long, env = "CLN_GRPC_CLIENT_CERT_PATH")]
	cln_grpc_client_cert_path: Option<Option<PathBuf>>,
	#[arg(long, env = "CLN_GRPC_CLIENT_KEY_PATH")]
	cln_grpc_client_key_path: Option<Option<PathBuf>>,
}

impl ConfigOpts {
	/// Verifies if the specified configuration is valid
	///
	/// It also checks if all required arguments are present.
	/// It should only be used on create.
	fn validate(&self) -> anyhow::Result<()> {
		if self.bitcoind_url.is_none() {
			bail!("The --bitcoind-url flag is mandatory.");
		}

		let with_user_pass = match (&self.bitcoind_rpc_user, &self.bitcoind_rpc_pass) {
			(Some(_), None) => bail!("Missing parameter --bitcoind-rpc-pass. This is required if --bitcoind-rpc-user is provided"),
			(None, Some(_)) => bail!("Missing parameter --bitcoind-rpc-user. This is required if --bitcoind-rpc-pass is provided"),
			(None, None) => false,
			(Some(_),Some(_)) => true,
		};

		if !with_user_pass && self.bitcoind_cookie.is_none() {
			bail!("Configuring authentication to bitcoind is mandatory. Specify either --bitcoind-cookie or (--bitcoind-rpc-user and --bitcoind-rpc-pass).");
		} else if with_user_pass & self.bitcoind_cookie.is_some() {
			bail!("Invalid configuration for authentication to bitcoind. Use either --bitcoind-cookie or (--bitcoind-rpc-user and --bitcoind-rpc-pass) but not both.")
		}


		let has_cln_config =
			self.cln_grpc_uri.is_some() ||
			self.cln_grpc_client_cert_path.is_some() ||
			self.cln_grpc_client_cert_path.is_some() ||
			self.cln_grpc_client_key_path.is_some();

		if has_cln_config {

			if self.cln_grpc_uri.is_none() {
				bail!("The --cln-grpc-uri parameter is required if another cln-parameter is provided.")
			}
			if self.cln_grpc_server_cert_path.is_none() {
				bail!("The --cln-grpc-server-cert-path parameter is required if another cln-parameter is provided.")
			}
			if self.cln_grpc_client_cert_path.is_none() {
				bail!("The --cln-grpc-client-cert-path parameter is required if another cln-parameter is provided.")
			}
			if self.cln_grpc_client_key_path.is_none() {
				bail!("the --cln-grpc-client-key-path parameter is required if another cln-parameter is provided.")
			}
		}

		Ok(())
	}

	fn merge(self, mut cfg: Config) -> anyhow::Result<Config> {

		if let Some(v) = self.bitcoind_url {
			cfg.bitcoind_url = v;
		}

		if let Some(v) = self.public_rpc_address {
			cfg.public_rpc_address = v.parse().context("public_rpc_address is invalid")?;
		}

		if let Some(v) = self.admin_rpc_address {
			if let Some(v) = v {
				cfg.admin_rpc_address = Some(v.parse().context("Invalid admin_rpc_address")?);
			} else {
				cfg.admin_rpc_address = None;
			}
		}

		if let Some(v) = self.round_interval {
			cfg.round_interval = Duration::from_millis(v);
		}

		if let Some(v) = self.round_submit_time {
			cfg.round_submit_time = Duration::from_millis(v);
		}

		if let Some(v) = self.round_sign_time {
			cfg.round_sign_time = Duration::from_millis(v);
		}

		if let Some(v) = self.nb_round_nonces {
			cfg.nb_round_nonces = v;
		}

		if let Some(v) = self.vtxo_expiry_delta {
			cfg.vtxo_expiry_delta = v;
		}

		if let Some(v) = self.vtxo_exit_delta {
			cfg.vtxo_exit_delta = v;
		}

		if let Some(v) = self.sweep_threshold {
			cfg.sweep_threshold = v;
		}

		if let Some(v) = self.round_tx_feerate_sat_per_kvb {
			cfg.round_tx_feerate = FeeRate::from_sat_per_kwu(
				(v.checked_sub(1).context("feerate can't be 0")? / 4) + 1
			);
		}

		// Merging authentication for bitcoind and validating
		// the resulting config
		let mut put_bitcoind_cookie = false;
		let mut put_bitcoind_userpass = false;
		if let Some(v) = self.bitcoind_cookie {
			put_bitcoind_cookie = true;
			cfg.bitcoind_cookie = Some(v);
			cfg.bitcoind_rpc_user = None;
			cfg.bitcoind_rpc_pass = None;
		}
		if let Some(v) = self.bitcoind_rpc_user {
			put_bitcoind_userpass = true;
			cfg.bitcoind_rpc_user = Some(v);
			cfg.bitcoind_cookie = None;
		}
		if let Some(v) = self.bitcoind_rpc_pass {
			put_bitcoind_userpass = true;
			cfg.bitcoind_rpc_pass = Some(v);
			cfg.bitcoind_cookie = None;
		}

		match (&cfg.bitcoind_rpc_user, &cfg.bitcoind_rpc_pass) {
			(Some(_), None) => bail!("Missing configuration for bitcoind-rpc-pass. This is required if bitcoind-rpc-user is provided"),
			(None, Some(_)) => bail!("Missing configuration for bitcoind-rpc-user. This is required if bitcoind-rpc-pass  is provided"),
			(Some(_), Some(_))=> true,
			(None, None) => false,
		};

		if put_bitcoind_userpass && put_bitcoind_cookie {
			bail!("Either set --bitcoind-cookie or (--bitcoind-rpc-user and --bitcoind-rpc-pass) but not both.")
		}


		// We have the following scenario's

		// If any of these fields is Some(Some(value)) it explcitily sets the field
		// In that case puts_cln_config is true
		let puts_cln_config =
			self.cln_grpc_uri.as_ref().map_or(false, |x| x.is_some()) ||
			self.cln_grpc_server_cert_path.as_ref().map_or(false, |x| x.is_some()) ||
			self.cln_grpc_client_cert_path.as_ref().map_or(false, |x| x.is_some()) ||
			self.cln_grpc_client_key_path.as_ref().map_or(false, |x| x.is_some());

		// If any of these fields is Some(None) it explicitly drops the field
		// In that case drops_cln_config is true
		let drops_some_cln_config =
			self.cln_grpc_uri.as_ref().map_or(false, |x| x.is_none()) ||
			self.cln_grpc_server_cert_path.as_ref().map_or(false, |x| x.is_none()) ||
			self.cln_grpc_client_cert_path.as_ref().map_or(false, |x| x.is_none()) ||
			self.cln_grpc_client_key_path.as_ref().map_or(false, |x| x.is_none());

		// If all of the fields are Some(None)
		let drops_all_cln_config =
			self.cln_grpc_uri.as_ref().map_or(false, |x| x.is_none()) &&
			self.cln_grpc_server_cert_path.as_ref().map_or(false, |x| x.is_none()) &&
			self.cln_grpc_client_cert_path.as_ref().map_or(false, |x| x.is_none()) &&
			self.cln_grpc_client_key_path.as_ref().map_or(false, |x| x.is_none());


		if cfg.cln_config.is_none() && puts_cln_config {
			// New cln-config is added
			cfg.cln_config = Some(ClnConfig {
				grpc_uri: self.cln_grpc_uri.clone().flatten().context("--cln-grpc-uri is required when a cln-config is provided")?,
				grpc_server_cert_path: self.cln_grpc_server_cert_path.clone().flatten().context("--cln-server-cert-path is required when a cln-config is provided")?,
				grpc_client_cert_path: self.cln_grpc_client_cert_path.clone().flatten().context("--cln-grpc-client-cert-path is required when a cln-config is provided")?,
				grpc_client_key_path: self.cln_grpc_client_key_path.clone().flatten().context("--cln-grpc-client-key-path is required when a cln-config is provided")?,
			});
		}
		if cfg.cln_config.is_none() {
			// Don't do anything
			// There is no config and we're not changing it
		}
		else if cfg.cln_config.is_some() && drops_all_cln_config {
			// The cln-config is dropped
			cfg.cln_config = None
		}
		else if cfg.cln_config.is_some() && drops_some_cln_config {
			bail!("Invalid configuration. Remove either all cln-config bariables or none")
		}
		else {
			let cln_config = cfg.cln_config.as_mut().unwrap();

			if let Some(Some(v)) = self.cln_grpc_uri {
				cln_config.grpc_uri = v;
			}
			if let Some(Some(v)) = self.cln_grpc_server_cert_path {
				cln_config.grpc_server_cert_path = v;
			}
			if let Some(Some(v)) = self.cln_grpc_client_cert_path {
				cln_config.grpc_client_cert_path = v;
			}
			if let Some(Some(v)) = self.cln_grpc_client_key_path {
				cln_config.grpc_client_key_path = v;
			}
		}

		Ok(cfg)
	}

}

#[cfg(test)]
mod test {
	use super::*;

	use std::str::FromStr;

	#[test]
	fn validate_bitcoind_config() {
		let bitcoind_url = Some(String::from("http://localhost:13444"));
		let bitcoind_cookie = Some(PathBuf::from("/path/to/cookie"));
		let bitcoind_rpc_user = Some(String::from("user"));
		let bitcoind_rpc_pass = Some(String::from("pass"));

		let mut opts = ConfigOpts::default();
		opts.bitcoind_url = bitcoind_url.clone();
		opts.bitcoind_cookie = bitcoind_cookie.clone();
		opts.validate().expect("This config should be valid");

		let mut opts = ConfigOpts::default();
		opts.bitcoind_url = bitcoind_url.clone();
		opts.bitcoind_rpc_user = bitcoind_rpc_user.clone();
		opts.bitcoind_rpc_pass = bitcoind_rpc_pass.clone();
		opts.validate().expect("This config should be valid");

		let mut opts = ConfigOpts::default();
		opts.bitcoind_url = bitcoind_url.clone();
		opts.validate().expect_err("Invalid because auth info is missing");

		let mut opts = ConfigOpts::default();
		opts.bitcoind_url = bitcoind_url.clone();
		opts.bitcoind_rpc_user = bitcoind_rpc_user.clone();
		opts.validate().expect_err("Invalid because pass is missing");

		let mut opts = ConfigOpts::default();
		opts.bitcoind_url = bitcoind_url.clone();
		opts.bitcoind_cookie = bitcoind_cookie.clone();
		opts.bitcoind_rpc_user = bitcoind_rpc_user.clone();
		opts.bitcoind_rpc_pass = bitcoind_rpc_pass.clone();
		opts.validate().expect_err("Invalid. Either cookie or pass but not both");
	}


	#[test]
	fn update_bitcoind_auth_config() {
		let bitcoind_cookie = Some(PathBuf::from("/path/to/cookie"));
		let bitcoind_rpc_user = Some(String::from("user"));
		let bitcoind_rpc_pass = Some(String::from("pass"));

		// Aspd is configured to use config
		// COnfigure to use user pass instead
		let mut config = Config::default();
		config.bitcoind_cookie = bitcoind_cookie.clone();

		let mut update = ConfigOpts::default();
		update.bitcoind_rpc_user = bitcoind_rpc_user.clone();
		update.bitcoind_rpc_pass = bitcoind_rpc_pass.clone();

		let new_config = update.merge(config).expect("Valid change");
		assert!(new_config.bitcoind_cookie.is_none());
		assert!(new_config.bitcoind_rpc_user.is_some());
		assert!(new_config.bitcoind_rpc_pass.is_some());
	}

	#[test]
	fn partial_cln_config_on_init_is_not_accepted() {
		let cfg = Config::default();

		// Create partial config opts
		let uri = Uri::from_str("http://localhost:1313").unwrap();
		let updates = ConfigOpts {
			cln_grpc_uri: Some(Some(uri)),
			..ConfigOpts::default()
		};

		updates.merge(cfg).expect_err("This should fail");
	}

	#[test]
	fn init_accepts_full_cln_config() {
		let cfg = Config::default();

		// Create full Config Opts
		let uri = Uri::from_str("http://localhost:1313").unwrap();
		let updates = ConfigOpts {
			cln_grpc_uri: Some(Some(uri.clone())),
			cln_grpc_server_cert_path: Some(Some(PathBuf::from("/certs/server.crt"))),
			cln_grpc_client_cert_path: Some(Some(PathBuf::from("/certs/client.crt"))),
			cln_grpc_client_key_path: Some(Some(PathBuf::from("/certs/client.key"))),
			..ConfigOpts::default()
		};

		let cfg = updates.merge(cfg).expect("Accepts full config");

		let cln_config = cfg.cln_config.as_ref().expect("A config has been created");
		assert_eq!(cln_config.grpc_uri, uri);
		assert_eq!(cln_config.grpc_server_cert_path, PathBuf::from("/certs/server.crt"));
		assert_eq!(cln_config.grpc_client_cert_path, PathBuf::from("/certs/client.crt"));
		assert_eq!(cln_config.grpc_client_key_path, PathBuf::from("/certs/client.key"));
	}

	#[test]
	fn drop_cln_config() {
		let mut cfg = Config::default();

		let uri = Uri::from_str("http://localhost:1313").unwrap();
		cfg.cln_config = Some(ClnConfig{
			grpc_uri: uri.clone(),
			grpc_server_cert_path: PathBuf::from("/certs/server.cert"),
			grpc_client_cert_path: PathBuf::from("/certs/client.cert"),
			grpc_client_key_path: PathBuf::from("/certs/client.key"),
		});

		// Create config opts that drop cln-config
		let updates = ConfigOpts {
			cln_grpc_uri: Some(None),
			cln_grpc_server_cert_path: Some(None),
			cln_grpc_client_cert_path: Some(None),
			cln_grpc_client_key_path: Some(None),
			..ConfigOpts::default()
		};

		let cfg = updates.merge(cfg).expect("Accepts full config");
		assert!(cfg.cln_config.is_none());
	}

	#[test]
	fn drop_partial_cln_config() {
		let mut cfg = Config::default();

		let uri : Uri = "http://localhost:1313".parse().unwrap();
		cfg.cln_config = Some(ClnConfig{
			grpc_uri: uri.clone(),
			grpc_server_cert_path: PathBuf::from("/certs/server.cert"),
			grpc_client_cert_path: PathBuf::from("/certs/client.cert"),
			grpc_client_key_path: PathBuf::from("/certs/client.key"),
		});

		// Creates config-opts that partially drop cln-config
		let updates = ConfigOpts {
			cln_grpc_uri: Some(None),
			..ConfigOpts::default()
		};

		updates.merge(cfg).expect_err("Cannot drop cln-config partially");
	}

	#[test]
	fn update_cln_config() {
		let mut cfg = Config::default();

		let uri = Uri::from_str("http://localhost:1313").unwrap();
		cfg.cln_config = Some(ClnConfig{
			grpc_uri: uri.clone(),
			grpc_server_cert_path: PathBuf::from("/certs/server.cert"),
			grpc_client_cert_path: PathBuf::from("/certs/client.cert"),
			grpc_client_key_path: PathBuf::from("/certs/client.key"),
		});

		// Set a cln-config
		let new_uri = Uri::from_str("http://otheruri").unwrap();
		let updates = ConfigOpts {
			cln_grpc_uri: Some(Some(new_uri.clone())),
			..ConfigOpts::default()
		};

		let cfg = updates.merge(cfg).expect("Can update config");

		assert_eq!(cfg.cln_config.unwrap().grpc_uri, new_uri);
	}
}
