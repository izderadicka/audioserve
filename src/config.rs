use clap::{App, Arg};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::env;
use std::io;
use super::services::transcode::{Quality, QualityLevel};
use num_cpus;
use serde_yaml;
use std::collections::BTreeMap;
use std::fs::File;

static mut CONFIG: Option<Config> = None;

// CONFIG is assured to be inited only once from main thread
pub fn get_config() -> &'static Config {
    unsafe {
        CONFIG.as_ref().expect("Config is not initialized")
    }
}

quick_error! { 
#[derive(Debug)]
pub enum Error {
    
    InvalidNumber(err: ::std::num::ParseIntError) {
        from()
    }

    InvalidAddress(err: ::std::net::AddrParseError) {
        from()
    }

    InvalidPath(err: io::Error) {
        from()
    }
    
    InvalidLimitValue(err: &'static str) {
        from()
    }

    InvalidYamlFile(err: serde_yaml::Error) {
        from()
    }

    NonExistentBaseDirectory{ }

    NonExistentClientDirectory{ }

    NonExistentSSLKeyFile { }
}
}

#[derive(Debug, Clone)]
pub struct TranscodingConfig {
    low: Option<Quality>,
    medium: Option<Quality>,
    high: Option<Quality>
}

impl TranscodingConfig {
    pub fn get(&self, quality: QualityLevel) -> Quality {
        match quality {
            l @ QualityLevel::Low => self.low.as_ref().map_or(Quality::default_level(l), |c| c.clone()),
            l @ QualityLevel::Medium => self.medium.as_ref().map_or(Quality::default_level(l), |c| c.clone()), 
            l @ QualityLevel::High => self.high.as_ref().map_or(Quality::default_level(l), |c| c.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config{
    pub local_addr: SocketAddr,
    pub max_sending_threads: usize,
    pub base_dirs: Vec<PathBuf>,
    pub shared_secret: Option<String>,
    pub transcoding: TranscodingConfig,
    pub max_transcodings: usize,
    pub token_validity_hours: u64,
    pub secret_file: PathBuf,
    pub client_dir: PathBuf,
    pub cors: bool,
    pub ssl_key_file: Option<PathBuf>,
    pub ssl_key_password: Option<String>

}
type Parser<'a> = App<'a, 'a>;

fn create_parser<'a>() -> Parser<'a> {
    App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .arg(Arg::with_name("debug")
            .short("d")
            .long("debug")
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable)")
        )
        .arg(Arg::with_name("local_addr")
            .short("l")
            .long("listen")
            .help("Address and port server is listening on as address:port")
            .takes_value(true)
            .default_value("0.0.0.0:3000")
        )
        .arg(Arg::with_name("max-threads")
            .short("m")
            .long("max-threads")
            .takes_value(true)
            .help("Maximum number of threads for requests processing")
            .default_value("100")
        )
        .arg(Arg::with_name("base_dir")
            .value_name("BASE_DIR")
            .required(true)
            .multiple(true)
            .min_values(1)
            .max_values(100)
            .takes_value(true)
            .help("Root directory for audio books")

        )
        .arg(Arg::with_name("no-authentication")
            .long("no-authentication")
            .help("no authentication required - mainly for testing purposes")
        )
        .arg(Arg::with_name("shared-secret")
            .short("s")
            .long("shared-secret")
            .takes_value(true)
            .required_unless("no-authentication")
            .help("Shared secret for client authentication")
        )
        .arg(Arg::with_name("transcoding-config")
            .long("transcoding-config")
            .takes_value(true)
            .help("Custom transcoding config in yaml file [defaul: built-in settings of 32, 48, 64 kbps")
        )
        .arg(Arg::with_name("max-transcodings")
            .short("x")
            .long("max-transcodings")
            .takes_value(true)
            .help("Maximum number of concurrent transcodings [default: number of cores]")
        )
        .arg(Arg::with_name("token-validity-hours")
            .long("token-validity-hours")
            .takes_value(true)
            .help("Validity of authentication token issued by this server in hours")
            .default_value("8760")
        )
        .arg(Arg::with_name("client-dir")
            .short("c")
            .long("client-dir")
            .takes_value(true)
            .help("Directory with client files - index.html and bundle.js")
            .default_value("./client/dist")
        )
        .arg(Arg::with_name("secret-file")
            .long("secret-file")
            .takes_value(true)
            .help("Path to file where server is kept - it's generated if it does not exists [default: is $HOME/.audioserve.secret]")
        )
        .arg(Arg::with_name("cors")
            .long("cors")
            .help("Enable CORS - enables any origin of requests")
        )
        .arg(Arg::with_name("ssl-key")
            .long("ssl-key")
            .takes_value(true)
            .help("TLS/SSL private key and certificate in form of PKCS#12 key file, if provided, https is used")
        )
        .arg(Arg::with_name("ssl-key-password")
            .long("ssl-key-password")
            .takes_value(true)
            .requires("ssl-key")
            .help("Password for TLS/SSL private key")
        )
}

pub fn parse_args() -> Result<(), Error>{
    let p = create_parser();
    let args = p.get_matches();

    if args.is_present("debug") {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, "debug");
        }
    }

    let base_dirs_items = args.values_of("base_dir").unwrap();
    let mut base_dirs = vec![];
    for dir in base_dirs_items {
        let base_dir: PathBuf = dir.into();
        if ! base_dir.is_dir() {
        return Err(Error::NonExistentBaseDirectory)
        }
        base_dirs.push(base_dir);

    }

    
    let local_addr = args.value_of("local_addr").unwrap().parse()?;
    let max_sending_threads = args.value_of("max-threads").unwrap().parse()?;
    if max_sending_threads < 10 {
        return Err("Too few threads - should be above 10".into())
    }
    if max_sending_threads > 10000 {
        return Err("Too much threads - should be below 10000".into())
    }
    
    let shared_secret = if args.is_present("no-authentication") {
        None } else {
        Some(args.value_of("shared-secret").unwrap().into())
        };

    // let transcoding = args.value_of("transcode").map(|t| match t {
    //     "low" => Quality::default_level(QualityLevel::Low),
    //     "medium" => Quality::default_level(QualityLevel::Medium),
    //     "high" => Quality::default_level(QualityLevel::High),
    //     _ => unreachable!("Wrong transcoding")
    // });

    
let transcoding = match  args.value_of("transcoding-config") {
    None =>  TranscodingConfig{low:None, medium:None, high:None},
    Some(f) => {
        let config_file = File::open(f)?;
        let mut qs: BTreeMap<String, Quality> = serde_yaml::from_reader(config_file)?; 
        TranscodingConfig{
            low:qs.remove("low"), 
            medium:qs.remove("medium"), 
            high:qs.remove("high")}
    }
};
    
   

    let max_transcodings = match args.value_of("max-transcodings") {
        Some(s) => {
            s.parse()?
        },
        None => {
            num_cpus::get()
        }
    };
    if max_transcodings < 1 {
        return Err("At least one concurrent trancoding must be available".into())
    } else if max_transcodings > max_sending_threads {
        return Err("Number of concurrent transcodings cannot be higher then number of threads".into())
    }

    let token_validity_hours = args.value_of("token-validity-hours").unwrap().parse()?;
    if token_validity_hours < 1 {
        return Err("Token must be valid for at least an hour".into())
    }
    let client_dir: PathBuf = args.value_of("client-dir").unwrap().into();
    if ! client_dir.exists() {
        return Err(Error::NonExistentClientDirectory)
    }

    let secret_file = match args.value_of("secret-file") {
        Some(s) => s.into(),
        None => match ::std::env::home_dir() {
            Some(home) => home.join(".audioserve.secret"),
            None => "./.audioserve.secret".into()
        }
    };

    let cors = args.is_present("cors");

    let ssl_key_file = match args.value_of("ssl-key") {
        Some(f) => {
            let p: PathBuf = f.into();
            if ! p.exists() {
                return Err(Error::NonExistentSSLKeyFile)
            }
            Some(p)
        },
        None => None
    };

    let ssl_key_password = args.value_of("ssl-key-password").map(|s| s.into());

    let config = Config{
        base_dirs,
        local_addr,
        max_sending_threads,
        shared_secret, 
        transcoding,
        max_transcodings,
        token_validity_hours,
        client_dir,
        secret_file,
        cors,
        ssl_key_file,
        ssl_key_password

    };
    unsafe {
        CONFIG = Some(config);
    }

    Ok(())

}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_yaml_serialize() {
        let mut qualities = BTreeMap::new();
        qualities.insert("low", Quality::default_level(QualityLevel::Low));
        qualities.insert("medium", Quality::default_level(QualityLevel::Medium));
        qualities.insert("high", Quality::default_level(QualityLevel::High));
        let s = serde_yaml::to_string(&qualities).unwrap();
        assert!(s.len() > 20);
        println!("{}",s);

        let des: BTreeMap<String, Quality> = serde_yaml::from_str(&s).unwrap();
        assert_eq!(des.get("medium"), qualities.get("medium"));

    }
    #[test]
    fn test_yaml_deserialize() {
        let f = File::open("./test_data/transcodings.yaml").unwrap();
        let des: BTreeMap<String, Quality> = serde_yaml::from_reader(f).unwrap();
        assert_eq!(3, des.len());
        assert!(des.get("high").is_some());
        
        
    }


    

}