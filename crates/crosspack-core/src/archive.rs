#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    Zip,
    TarGz,
    TarZst,
    Bin,
    Msi,
    Dmg,
    AppImage,
    Exe,
    Pkg,
    Msix,
    Appx,
}

impl ArchiveType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
            Self::Bin => "bin",
            Self::Msi => "msi",
            Self::Dmg => "dmg",
            Self::AppImage => "appimage",
            Self::Exe => "exe",
            Self::Pkg => "pkg",
            Self::Msix => "msix",
            Self::Appx => "appx",
        }
    }

    pub fn cache_extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
            Self::Bin => "bin",
            Self::Msi => "msi",
            Self::Dmg => "dmg",
            Self::AppImage => "appimage",
            Self::Exe => "exe",
            Self::Pkg => "pkg",
            Self::Msix => "msix",
            Self::Appx => "appx",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "zip" => Some(Self::Zip),
            "tar.gz" | "tgz" => Some(Self::TarGz),
            "tar.zst" | "tzst" => Some(Self::TarZst),
            "bin" => Some(Self::Bin),
            "msi" => Some(Self::Msi),
            "dmg" => Some(Self::Dmg),
            "appimage" => Some(Self::AppImage),
            "exe" => Some(Self::Exe),
            "pkg" => Some(Self::Pkg),
            "msix" => Some(Self::Msix),
            "appx" => Some(Self::Appx),
            _ => None,
        }
    }

    pub fn infer_from_url(url: &str) -> Option<Self> {
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".zip") {
            return Some(Self::Zip);
        }
        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            return Some(Self::TarGz);
        }
        if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
            return Some(Self::TarZst);
        }
        if lower.ends_with(".bin") {
            return Some(Self::Bin);
        }
        if lower.ends_with(".msi") {
            return Some(Self::Msi);
        }
        if lower.ends_with(".dmg") {
            return Some(Self::Dmg);
        }
        if lower.ends_with(".appimage") {
            return Some(Self::AppImage);
        }
        if lower.ends_with(".exe") {
            return Some(Self::Exe);
        }
        if lower.ends_with(".pkg") {
            return Some(Self::Pkg);
        }
        if lower.ends_with(".msix") {
            return Some(Self::Msix);
        }
        if lower.ends_with(".appx") {
            return Some(Self::Appx);
        }

        let without_fragment = lower.split('#').next().unwrap_or(&lower);
        let without_query = without_fragment
            .split('?')
            .next()
            .unwrap_or(without_fragment);
        let file_name = without_query.rsplit('/').next().unwrap_or("");
        if !file_name.is_empty() && !file_name.contains('.') {
            return Some(Self::Bin);
        }

        None
    }
}
