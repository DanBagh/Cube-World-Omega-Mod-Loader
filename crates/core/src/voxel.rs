//! Voxel models: the game's `.cub` files and MagicaVoxel's `.vox`.
//!
//! A `.cub` is three little-endian u32 sizes (x, y, z) followed by x*y*z RGB triples, x
//! running fastest and z (up) slowest. Black means empty. MagicaVoxel is z-up too, so
//! models convert axis for axis.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub type Rgb = [u8; 3];

/// Cells the game loads from one model. It reads no further.
pub const MAX_CELLS: u64 = 0x20000;
const MAX_SIDE: u32 = 1024;
/// The game leaves voxels of these colors out of a model when it loads it.
const KEY_COLORS: [Rgb; 3] = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub size: [u32; 3],
    /// Color of each filled cell, by (x, y, z).
    pub voxels: BTreeMap<[u32; 3], Rgb>,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

impl Model {
    /// Reads a `.cub` or `.vox` file, going by its extension.
    pub fn load(path: &Path) -> Result<Model, String> {
        let kind = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let data = || fs::read(path).map_err(|e| e.to_string());
        match kind.as_str() {
            "cub" => Model::from_cub(&data()?),
            "vox" => Model::from_vox(&data()?),
            _ => Err("unsupported model type (use .cub or .vox)".into()),
        }
    }

    pub fn cells(&self) -> u64 {
        self.size.iter().map(|&side| u64::from(side)).product()
    }

    /// Voxels the game would drop because their color is pure red, green or blue.
    pub fn keyed(&self) -> usize {
        self.voxels
            .values()
            .filter(|rgb| KEY_COLORS.contains(rgb))
            .count()
    }

    pub fn from_cub(data: &[u8]) -> Result<Model, String> {
        let side = |at| u32_at(data, at).ok_or("not a .cub file");
        let size = [side(0)?, side(4)?, side(8)?];
        let cells: u64 = size.iter().map(|&s| u64::from(s)).product();
        if size.iter().any(|&s| s == 0 || s > MAX_SIDE) || data.len() as u64 != 12 + 3 * cells {
            return Err("not a .cub file (size header does not match the file length)".into());
        }
        let mut voxels = BTreeMap::new();
        for (index, rgb) in data[12..].as_chunks::<3>().0.iter().enumerate() {
            if *rgb != [0, 0, 0] {
                let index = index as u32;
                voxels.insert(
                    [
                        index % size[0],
                        index / size[0] % size[1],
                        index / (size[0] * size[1]),
                    ],
                    *rgb,
                );
            }
        }
        Ok(Model { size, voxels })
    }

    pub fn to_cub(&self) -> Vec<u8> {
        let [sx, sy, sz] = self.size.map(|side| side as usize);
        let mut out = vec![0u8; 12 + 3 * sx * sy * sz];
        for (at, side) in self.size.iter().enumerate() {
            out[4 * at..4 * at + 4].copy_from_slice(&side.to_le_bytes());
        }
        for (&[x, y, z], rgb) in &self.voxels {
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x < sx && y < sy && z < sz {
                let at = 12 + 3 * (x + sx * (y + sy * z));
                // black would read as empty
                out[at..at + 3].copy_from_slice(if *rgb == [0, 0, 0] { &[1, 1, 1] } else { rgb });
            }
        }
        out
    }

    /// The first model of a MagicaVoxel file, with its palette applied.
    pub fn from_vox(data: &[u8]) -> Result<Model, String> {
        if data.get(..4) != Some(b"VOX ") || data.get(8..12) != Some(b"MAIN") {
            return Err("not a MagicaVoxel .vox file".into());
        }
        let (mut size, mut cells, mut palette) = (None, None, None);
        let mut at = 20;
        while let (Some(kind), Some(length), Some(children)) = (
            data.get(at..at + 4),
            u32_at(data, at + 4),
            u32_at(data, at + 8),
        ) {
            let Some(body) = data.get(at + 12..at + 12 + length as usize) else {
                break;
            };
            match kind {
                b"SIZE" if size.is_none() => {
                    size = Some([u32_at(body, 0), u32_at(body, 4), u32_at(body, 8)])
                }
                b"XYZI" if cells.is_none() => {
                    let count = u32_at(body, 0).unwrap_or(0) as usize;
                    cells = body.get(4..4 + 4 * count);
                }
                b"RGBA" => palette = Some(body),
                _ => {}
            }
            at += 12 + length as usize + children as usize;
        }
        let (Some([Some(x), Some(y), Some(z)]), Some(cells)) = (size, cells) else {
            return Err("the .vox file contains no model".into());
        };
        let palette = palette
            .filter(|p| p.len() >= 1020)
            .ok_or("the .vox file has no palette; open and re-save it in MagicaVoxel")?;
        let mut voxels = BTreeMap::new();
        for &[x, y, z, color] in cells.as_chunks::<4>().0 {
            if color != 0 {
                let entry = 4 * (color as usize - 1);
                voxels.insert(
                    [u32::from(x), u32::from(y), u32::from(z)],
                    [palette[entry], palette[entry + 1], palette[entry + 2]],
                );
            }
        }
        Ok(Model {
            size: [x, y, z],
            voxels,
        })
    }

    pub fn to_vox(&self) -> Result<Vec<u8>, String> {
        if self.size.iter().any(|&side| side > 256) {
            return Err("MagicaVoxel models are at most 256 voxels per side".into());
        }
        let mut colors: Vec<Rgb> = Vec::new();
        let mut cells = Vec::with_capacity(4 * self.voxels.len());
        for (&[x, y, z], rgb) in &self.voxels {
            let index = colors.iter().position(|c| c == rgb).unwrap_or_else(|| {
                colors.push(*rgb);
                colors.len() - 1
            });
            cells.extend_from_slice(&[x as u8, y as u8, z as u8, index as u8 + 1]);
        }
        if colors.len() > 255 {
            return Err("more than 255 different colors; MagicaVoxel palettes hold 255".into());
        }
        let chunk = |kind: &[u8; 4], body: &[u8]| {
            [
                kind.as_slice(),
                &(body.len() as u32).to_le_bytes(),
                &[0; 4],
                body,
            ]
            .concat()
        };
        let size: Vec<u8> = self
            .size
            .iter()
            .flat_map(|side| side.to_le_bytes())
            .collect();
        let xyzi = [&(self.voxels.len() as u32).to_le_bytes(), cells.as_slice()].concat();
        let mut rgba: Vec<u8> = colors
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect();
        rgba.resize(1024, 0);
        let children = [
            chunk(b"SIZE", &size),
            chunk(b"XYZI", &xyzi),
            chunk(b"RGBA", &rgba),
        ]
        .concat();
        Ok([
            b"VOX ".as_slice(),
            &150u32.to_le_bytes(),
            b"MAIN",
            &[0; 4],
            &(children.len() as u32).to_le_bytes(),
            &children,
        ]
        .concat())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 2x1x3 model with two colors and empty cells.
    pub(crate) fn sample() -> Model {
        Model {
            size: [2, 1, 3],
            voxels: BTreeMap::from([
                ([0, 0, 0], [10, 20, 30]),
                ([1, 0, 0], [10, 20, 30]),
                ([1, 0, 2], [200, 100, 0]),
            ]),
        }
    }

    #[test]
    fn cub_round_trip() {
        let cub = sample().to_cub();
        assert_eq!(cub.len(), 12 + 3 * 6);
        assert_eq!(&cub[..12], &[2, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0]);
        assert_eq!(&cub[12..18], &[10, 20, 30, 10, 20, 30]);
        assert_eq!(&cub[12 + 3 * 5..], &[200, 100, 0]);
        assert_eq!(Model::from_cub(&cub).unwrap(), sample());
    }

    #[test]
    fn vox_round_trip() {
        let vox = sample().to_vox().unwrap();
        assert_eq!(Model::from_vox(&vox).unwrap(), sample());
    }

    #[test]
    fn black_voxels_survive_as_near_black() {
        let mut model = sample();
        model.voxels.insert([0, 0, 1], [0, 0, 0]);
        let read = Model::from_cub(&model.to_cub()).unwrap();
        assert_eq!(read.voxels[&[0, 0, 1]], [1, 1, 1]);
    }

    #[test]
    fn rejects_files_that_are_not_models() {
        assert!(Model::from_cub(b"short").is_err());
        assert!(
            Model::from_cub(&[&[2u8, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0][..], &[0; 5]].concat())
                .is_err()
        );
        assert!(Model::from_cub(&[0; 12]).is_err());
        assert!(Model::from_vox(b"not a vox file at all").is_err());
        let mut no_palette = sample().to_vox().unwrap();
        no_palette.truncate(no_palette.len() - 1036);
        assert!(
            Model::from_vox(&no_palette)
                .unwrap_err()
                .contains("palette")
        );
    }

    #[test]
    fn counts_cells_and_key_colors() {
        let mut model = sample();
        assert_eq!((model.cells(), model.keyed()), (6, 0));
        model.voxels.insert([0, 0, 1], [0, 255, 0]);
        assert_eq!(model.keyed(), 1);
    }

    #[test]
    fn loads_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.CUB"), sample().to_cub()).unwrap();
        fs::write(dir.path().join("a.vox"), sample().to_vox().unwrap()).unwrap();
        fs::write(dir.path().join("a.txt"), b"x").unwrap();
        assert_eq!(Model::load(&dir.path().join("a.CUB")).unwrap(), sample());
        assert_eq!(Model::load(&dir.path().join("a.vox")).unwrap(), sample());
        assert!(Model::load(&dir.path().join("a.txt")).is_err());
    }
}
