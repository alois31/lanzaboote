use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use goblin::pe::PE;
use sha2::{Digest, Sha256};

use crate::utils::SecureTempDirExt;

type Hash = sha2::digest::Output<Sha256>;

/// Attach all information that lanzaboote needs into the PE binary.
///
/// When this function is called the referenced files already need to
/// be present in the ESP. This is required, because we need to read
/// them to compute hashes.
pub fn lanzaboote_image(
    // Because the returned path of this function is inside the tempdir as well, the tempdir must
    // live longer than the function. This is why it cannot be created inside the function.
    tempdir: &tempfile::TempDir,
    lanzaboote_stub: &Path,
    os_release: &Path,
    kernel_cmdline: &[String],
    kernel_path: &Path,
    initrd_path: &Path,
    esp: &Path,
) -> Result<PathBuf> {
    // objcopy can only copy files into the PE binary. That's why we
    // have to write the contents of some bootspec properties to disk.
    let kernel_cmdline_file =
        tempdir.write_secure_file("kernel-cmdline", kernel_cmdline.join(" "))?;

    let kernel_path_file =
        tempdir.write_secure_file("kernel-path", esp_relative_uefi_path(esp, kernel_path)?)?;
    let kernel_hash_file =
        tempdir.write_secure_file("kernel-hash", file_hash(kernel_path)?.as_slice())?;

    let initrd_path_file =
        tempdir.write_secure_file("initrd-path", esp_relative_uefi_path(esp, initrd_path)?)?;
    let initrd_hash_file =
        tempdir.write_secure_file("initrd-hash", file_hash(initrd_path)?.as_slice())?;

    let os_release_offs = stub_offset(lanzaboote_stub)?;
    let kernel_cmdline_offs = os_release_offs + file_size(os_release)?;
    let initrd_path_offs = kernel_cmdline_offs + file_size(&kernel_cmdline_file)?;
    let kernel_path_offs = initrd_path_offs + file_size(&initrd_path_file)?;
    let initrd_hash_offs = kernel_path_offs + file_size(&kernel_path_file)?;
    let kernel_hash_offs = initrd_hash_offs + file_size(&initrd_hash_file)?;

    let sections = vec![
        s(".osrel", os_release, os_release_offs),
        s(".cmdline", kernel_cmdline_file, kernel_cmdline_offs),
        s(".initrdp", initrd_path_file, initrd_path_offs),
        s(".kernelp", kernel_path_file, kernel_path_offs),
        s(".initrdh", initrd_hash_file, initrd_hash_offs),
        s(".kernelh", kernel_hash_file, kernel_hash_offs),
    ];

    let image_path = tempdir.path().join("lanzaboote-stub.efi");
    wrap_in_pe(lanzaboote_stub, sections, &image_path)?;
    Ok(image_path)
}

/// Compute the SHA 256 hash of a file.
fn file_hash(file: &Path) -> Result<Hash> {
    Ok(Sha256::digest(fs::read(file)?))
}

/// Take a PE binary stub and attach sections to it.
///
/// The resulting binary is then written to a newly created file at the provided output path.
fn wrap_in_pe(stub: &Path, sections: Vec<Section>, output: &Path) -> Result<()> {
    let mut args: Vec<OsString> = sections.iter().flat_map(Section::to_objcopy).collect();

    [stub.as_os_str(), output.as_os_str()]
        .iter()
        .for_each(|a| args.push(a.into()));

    let status = Command::new("objcopy")
        .args(&args)
        .status()
        .context("Failed to run objcopy command")?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to wrap in pe with args `{:?}`",
            &args
        ));
    }

    Ok(())
}

struct Section {
    name: &'static str,
    file_path: PathBuf,
    offset: u64,
}

impl Section {
    /// Create objcopy `-add-section` command line parameters that
    /// attach the section to a PE file.
    fn to_objcopy(&self) -> Vec<OsString> {
        // There is unfortunately no format! for OsString, so we cannot
        // just format a path.
        let mut map_str: OsString = format!("{}=", self.name).into();
        map_str.push(&self.file_path);

        vec![
            OsString::from("--add-section"),
            map_str,
            OsString::from("--change-section-vma"),
            format!("{}={:#x}", self.name, self.offset).into(),
        ]
    }
}

fn s(name: &'static str, file_path: impl AsRef<Path>, offset: u64) -> Section {
    Section {
        name,
        file_path: file_path.as_ref().into(),
        offset,
    }
}

/// Convert a path to an UEFI path relative to the specified ESP.
fn esp_relative_uefi_path(esp: &Path, path: &Path) -> Result<String> {
    let relative_path = path
        .strip_prefix(esp)
        .with_context(|| format!("Failed to strip esp prefix: {:?} from: {:?}", esp, path))?;
    let uefi_path = uefi_path(relative_path)?;
    Ok(format!("\\{}", &uefi_path))
}

/// Convert a path to a UEFI string representation.
///
/// This might not _necessarily_ produce a valid UEFI path, since some UEFI implementations might
/// not support UTF-8 strings. A Rust String, however, is _always_ valid UTF-8.
fn uefi_path(path: &Path) -> Result<String> {
    path.to_str()
        .to_owned()
        .map(|x| x.replace('/', "\\"))
        .with_context(|| format!("Failed to convert {:?} to an UEFI path", path))
}

fn stub_offset(binary: &Path) -> Result<u64> {
    let pe_binary = fs::read(binary).context("Failed to read PE binary file")?;
    let pe = PE::parse(&pe_binary).context("Failed to parse PE binary file")?;

    let image_base = image_base(&pe);

    // The Virtual Memory Address (VMA) is relative to the image base, aka the image base
    // needs to be added to the virtual address to get the actual (but still virtual address)
    Ok(u64::from(
        pe.sections
            .last()
            .map(|s| s.virtual_size + s.virtual_address)
            .expect("Failed to calculate offset"),
    ) + image_base)
}

fn image_base(pe: &PE) -> u64 {
    pe.header
        .optional_header
        .expect("Failed to find optional header, you're fucked")
        .windows_fields
        .image_base
}

fn file_size(path: impl AsRef<Path>) -> Result<u64> {
    Ok(fs::metadata(&path)
        .with_context(|| {
            format!(
                "Failed to read file metadata to calculate its size: {:?}",
                path.as_ref()
            )
        })?
        .size())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_to_valid_uefi_path_relative_to_esp() {
        let esp = Path::new("esp");
        let path = Path::new("esp/lanzaboote/is/great.txt");
        let converted_path = esp_relative_uefi_path(esp, path).unwrap();
        let expected_path = String::from("\\lanzaboote\\is\\great.txt");
        assert_eq!(converted_path, expected_path);
    }

    #[test]
    fn convert_to_valid_uefi_path() {
        let path = Path::new("lanzaboote/is/great.txt");
        let converted_path = uefi_path(path).unwrap();
        let expected_path = String::from("lanzaboote\\is\\great.txt");
        assert_eq!(converted_path, expected_path);
    }
}
