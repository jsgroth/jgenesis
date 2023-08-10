use env_logger::Env;
use std::env;

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut args = env::args();
    args.next();

    smsgg_core::run(&args.next().unwrap());
}
