use std::io::Read;
use std::sync::OnceLock;

use anyhow::{ensure, Context, Result};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub package: String,
    pub version: String,
    pub file_version: String,
    assets: Vec<AssetMetadata>,
}

#[derive(Deserialize)]
struct AssetMetadata {
    path: String,
    size: u64,
    sha256: String,
}

pub(super) struct Asset {
    pub key: &'static str,
    pub path: &'static str,
    pub compressed: &'static [u8],
}

pub(super) fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/conpty/manifest.json"))
            .expect("valid bundled ConPTY manifest")
    })
}

impl Asset {
    pub fn decode(&self) -> Result<Vec<u8>> {
        decode(self.key, self.compressed).with_context(|| format!("decode bundled {}", self.path))
    }

    pub fn verify(&self, reader: impl Read) -> Result<()> {
        let metadata = metadata(self.key)?;
        let mut reader = reader.take(metadata.size + 1);
        let mut hash = Sha256::new();
        let size = std::io::copy(&mut reader, &mut hash)?;
        ensure!(
            size == metadata.size && format!("{:x}", hash.finalize()) == metadata.sha256,
            "bundled {} checksum mismatch",
            self.path
        );
        Ok(())
    }
}

fn metadata(key: &str) -> Result<&'static AssetMetadata> {
    manifest()
        .assets
        .iter()
        .find(|asset| asset.path == key)
        .with_context(|| format!("missing ConPTY metadata for {key}"))
}

fn decode(key: &str, compressed: &[u8]) -> Result<Vec<u8>> {
    let metadata = metadata(key)?;
    let mut bytes = Vec::with_capacity(metadata.size as usize);
    GzDecoder::new(compressed)
        .take(metadata.size + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == metadata.size
            && format!("{:x}", Sha256::digest(&bytes)) == metadata.sha256,
        "bundled {key} checksum mismatch"
    );
    Ok(bytes)
}

macro_rules! asset {
    ($architecture:literal, $file:literal, $path:literal) => {
        Asset {
            key: concat!($architecture, "/", $file),
            path: $path,
            compressed: include_bytes!(concat!(
                "../../assets/conpty/",
                $architecture,
                "/",
                $file,
                ".gz"
            )),
        }
    };
}

#[cfg(windows)]
pub(super) fn selected() -> &'static [Asset] {
    #[cfg(target_arch = "x86_64")]
    const ASSETS: &[Asset] = &[
        asset!("x64", "conpty.dll", "conpty.dll"),
        asset!("x64", "OpenConsole.exe", "x64/OpenConsole.exe"),
        asset!("arm64", "OpenConsole.exe", "arm64/OpenConsole.exe"),
    ];
    #[cfg(target_arch = "aarch64")]
    const ASSETS: &[Asset] = &[
        asset!("arm64", "conpty.dll", "conpty.dll"),
        asset!("arm64", "OpenConsole.exe", "arm64/OpenConsole.exe"),
    ];
    #[cfg(target_arch = "x86")]
    const ASSETS: &[Asset] = &[
        asset!("x86", "conpty.dll", "conpty.dll"),
        asset!("x86", "OpenConsole.exe", "x86/OpenConsole.exe"),
        asset!("x64", "OpenConsole.exe", "x64/OpenConsole.exe"),
        asset!("arm64", "OpenConsole.exe", "arm64/OpenConsole.exe"),
    ];
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    const ASSETS: &[Asset] = &[];
    ASSETS
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    #[test]
    fn conpty_assets_match_the_pinned_manifest() {
        let assets = [
            asset!("x86", "conpty.dll", "conpty.dll"),
            asset!("x86", "OpenConsole.exe", "x86/OpenConsole.exe"),
            asset!("x64", "conpty.dll", "conpty.dll"),
            asset!("x64", "OpenConsole.exe", "x64/OpenConsole.exe"),
            asset!("arm64", "conpty.dll", "conpty.dll"),
            asset!("arm64", "OpenConsole.exe", "arm64/OpenConsole.exe"),
        ];
        assert_eq!(manifest().assets.len(), assets.len());
        for asset in assets {
            let bytes = asset.decode().unwrap();
            asset.verify(bytes.as_slice()).unwrap();
        }
        assert!(!manifest().package.is_empty());
        assert!(!manifest().version.is_empty());
        assert!(!manifest().file_version.is_empty());
    }

    #[test]
    fn conpty_rejects_changed_or_oversized_assets() {
        let asset = asset!("x64", "conpty.dll", "conpty.dll");
        let mut bytes = asset.decode().unwrap();
        bytes[0] ^= 1;
        for data in [&bytes[..], &[bytes.as_slice(), b"x"].concat()[..]] {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(data).unwrap();
            let compressed = encoder.finish().unwrap();
            assert!(decode(asset.key, &compressed).is_err());
        }
    }
}
