//! EM1 REL 0x246c84 / 0x246f78: inner-surface visibility on piece transitions.
//! This state is distinct from collision material state and shader progress.
use super::jigsaw::Piece;
use anyhow::{ensure, Result};

#[derive(Debug)]
pub struct Topology {
    thinned: Vec<bool>,
    exposed_neighbors: Vec<u32>,
}
impl Topology {
    pub fn new(piece_count: usize) -> Self {
        Self {
            thinned: vec![false; piece_count],
            exposed_neighbors: vec![0; piece_count],
        }
    }
    /// Emit secondary-mesh visibility writes in native order. The caller must
    /// supply the original directed adjacency, including repeated entries.
    /// Initial saved modifications must be replayed by the initialization path;
    /// material paint bits alone do not establish transition ordering.
    pub fn set_thinned(
        &mut self,
        pieces: &[Piece],
        adjacency: &[u16],
        piece: usize,
        thinned: bool,
        mut visibility: impl FnMut(u32, bool),
    ) -> Result<()> {
        ensure!(
            pieces.len() == self.thinned.len() && piece < pieces.len(),
            "Invalid jigsaw topology piece"
        );
        if self.thinned[piece] == thinned {
            return Ok(());
        }
        let source = &pieces[piece];
        // The original narrows the adjacency lookup index to u16 on each call.
        // Validate the full operation before mutating state or emitting writes.
        for offset in 0..source.adjacency_count {
            let index = source.adjacency_start.wrapping_add(offset as u16) as usize;
            ensure!(
                adjacency
                    .get(index)
                    .is_some_and(|&p| (p as usize) < pieces.len()),
                "Invalid jigsaw neighbor"
            );
        }
        let delta = if thinned { 1 } else { u32::MAX };
        for offset in 0..source.adjacency_count {
            let index = source.adjacency_start.wrapping_add(offset as u16) as usize;
            self.adjust(pieces, adjacency[index] as usize, delta, &mut visibility);
        }
        self.adjust(pieces, piece, 0, &mut visibility);
        self.thinned[piece] = thinned;
        // REL 0x2472e8 also updates a separate registration service here.
        // Its purpose is not yet established; this module only owns visibility.
        Ok(())
    }
    fn adjust(
        &mut self,
        pieces: &[Piece],
        piece: usize,
        delta: u32,
        visibility: &mut impl FnMut(u32, bool),
    ) {
        let old = self.exposed_neighbors[piece];
        let new = old.wrapping_add(delta);
        self.exposed_neighbors[piece] = new;
        if new != 0 && old != 0 && delta != 0 {
            return;
        }
        for &mesh in &pieces[piece].secondary_mesh_links {
            visibility(mesh, new != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn piece(start: u16, count: u32, mesh: u32) -> Piece {
        Piece {
            adjacency_start: start,
            adjacency_count: count,
            mesh_links: vec![],
            secondary_mesh_links: vec![mesh],
            collision_keys: vec![],
            position: [0.; 3],
        }
    }
    #[test]
    fn shared_inner_surface_stays_visible_until_both_neighbors_return() -> Result<()> {
        let pieces = [piece(0, 1, 10), piece(1, 1, 11), piece(2, 0, 12)];
        let mut state = Topology::new(3);
        let mut writes = vec![];
        for (piece, thin) in [(0, true), (1, true), (0, false), (1, false)] {
            state.set_thinned(&pieces, &[2, 2], piece, thin, |mesh, on| {
                writes.push((mesh, on))
            })?;
        }
        assert_eq!(
            writes,
            [
                (12, true),
                (10, false),
                (11, false),
                (10, false),
                (12, false),
                (11, false)
            ]
        );
        assert_eq!(state.exposed_neighbors, [0, 0, 0]);
        Ok(())
    }
    #[test]
    fn directed_duplicates_and_same_state_are_preserved() -> Result<()> {
        let pieces = [piece(0, 2, 10), piece(2, 0, 11)];
        let mut state = Topology::new(2);
        let mut writes = vec![];
        state.set_thinned(&pieces, &[1, 1], 0, true, |m, v| writes.push((m, v)))?;
        assert_eq!(state.exposed_neighbors, [0, 2]);
        state.set_thinned(&pieces, &[1, 1], 0, true, |m, v| writes.push((m, v)))?;
        assert_eq!(writes, [(11, true), (10, false)]);
        state.set_thinned(&pieces, &[1, 1], 0, false, |m, v| writes.push((m, v)))?;
        assert_eq!(writes, [(11, true), (10, false), (11, false), (10, false)]);
        Ok(())
    }
    #[test]
    fn invalid_neighbor_does_not_partially_mutate() -> Result<()> {
        let pieces = [piece(0, 2, 10), piece(2, 0, 11)];
        let mut state = Topology::new(2);
        assert!(state
            .set_thinned(&pieces, &[1, 9], 0, true, |_, _| panic!("partial write"))
            .is_err());
        assert_eq!(state.exposed_neighbors, [0, 0]);
        assert_eq!(state.thinned, [false, false]);
        Ok(())
    }
}
