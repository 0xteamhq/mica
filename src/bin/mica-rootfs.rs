//! `mica-rootfs` — converts an OCI image into the rootfs + kernel
//! bundle that Firecracker / Cloud Hypervisor / Kata consume (P4.6).
//!
//! Status: scaffold. The plan is:
//!   1. Pull the OCI image referenced on the command line via the
//!      registry client.
//!   2. Flatten layers into an `ext4` filesystem image.
//!   3. Bundle a known-good `vmlinux` for the requested arch.
//!   4. Push the result back to the registry as a single OCI artifact
//!      using `super::isolation::snapshot` media type.
//!
//! Today, this CLI just prints what it would do — enough that the
//! `--isolation=firecracker` capability probe can sanity-check the
//! tool exists, and ops can dry-run the contract.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mica-rootfs",
    version,
    about = "Build rootfs + kernel bundles for mica's microVM drivers"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a rootfs bundle from an OCI image reference.
    Build {
        /// OCI image to flatten, e.g. `ghcr.io/0xteamhq/mica/chrome-headless-shell:126`.
        image: String,
        /// Output OCI artifact reference. Pushed to the same registry.
        #[arg(long)]
        out: String,
        /// Target architecture (`amd64` | `arm64`).
        #[arg(long, default_value = "amd64")]
        arch: String,
    },
    /// Print the kernel cmdline mica injects for headless workloads.
    Cmdline,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Build { image, out, arch } => {
            eprintln!("mica-rootfs build: scaffold only.");
            eprintln!("  image: {image}");
            eprintln!("  out:   {out}");
            eprintln!("  arch:  {arch}");
            eprintln!();
            eprintln!("Implementation tracked under P4.6:");
            eprintln!("  1. pull {image} from the registry");
            eprintln!("  2. flatten layers into rootfs.ext4");
            eprintln!("  3. attach a vmlinux for {arch}");
            eprintln!("  4. push as an OCI artifact ({out}) using");
            eprintln!("     application/vnd.mica.rootfs.v1+json.");
            std::process::exit(2);
        }
        Cmd::Cmdline => {
            // What we'll boot the microVM with — small, headless.
            println!(
                "console=ttyS0 reboot=k panic=1 pci=off random.trust_cpu=on quiet i8042.noaux=Y i8042.nomux=Y"
            );
        }
    }
}
