use clap::Clap;

mod build_pkgs;
mod collect;
mod compile;
mod distribute_native_extensions;
mod prepare_release;

#[derive(Clap)]
enum Args {
	#[clap(name = "compile")]
	Compile(self::compile::Args),
	#[clap(name = "collect")]
	Collect(self::collect::Args),
	#[clap(name = "distribute_native_extensions")]
	DistributeNativeExtensions(self::distribute_native_extensions::Args),
	#[clap(name = "prepare_release")]
	PrepareRelease(self::prepare_release::Args),
	#[clap(name = "build_pkgs")]
	BuildPkgs(self::build_pkgs::Args),
}

fn main() {
	let args = Args::parse();
	match args {
		Args::Compile(args) => self::compile::run(args),
		Args::Collect(args) => self::collect::run(args),
		Args::DistributeNativeExtensions(args) => self::distribute_native_extensions::run(args),
		Args::PrepareRelease(args) => self::prepare_release::run(args),
		Args::BuildPkgs(args) => self::build_pkgs::run(args),
	};
}