//! Prediction runs the ordinary gameplay tick on a disposable player-view world.
use super::*;

impl World {
    /// Reuse ordinary order validation for predictable player actions only.
    pub fn predict_command(&mut self, command: &Command) -> Result<()> {
        ensure!(
            self.view.is_some(),
            "prediction requires a filtered player view"
        );
        if command.player != self.view_player() {
            return Ok(());
        }
        let supported = match &command.order {
            Order::Move { .. }
            | Order::Stop { .. }
            | Order::Hold { .. }
            | Order::Attack { .. }
            | Order::AttackMove { .. }
            | Order::Patrol { .. }
            | Order::Gather { .. } => true,
            Order::Queue { order, .. } => matches!(
                order,
                UnitOrder::Move { .. }
                    | UnitOrder::AttackMove { .. }
                    | UnitOrder::Gather { .. }
                    | UnitOrder::Patrol { .. }
            ),
            _ => false,
        };
        if supported {
            self.apply(command);
        }
        Ok(())
    }

    /// Run the same gameplay tick as the server against the last disclosed state.
    /// AI, mission scripts and visibility remain server-owned. Speculative results
    /// are discarded when the next confirmed baseline arrives.
    pub fn predict_step(&mut self) -> Result<()> {
        ensure!(
            self.view.is_some(),
            "prediction requires a filtered player view"
        );
        ensure!(self.tick().0 < u64::MAX, "simulation tick exhausted");
        if self.state.winner.is_some() {
            return Ok(());
        }
        // Allocation sites defer new identities to the server. Do not change
        // gameplay state to an artificial entity-limit value: the same world
        // also serves ordinary command and placement validation.
        self.state
            .statistics
            .resize_with(self.state.players.len(), Default::default);
        let winner = self.state.winner;
        let defeated = self.state.defeated.clone();
        self.advance_gameplay();
        // An incomplete player view cannot decide whether a player is eliminated.
        self.state.winner = winner;
        self.state.defeated = defeated;
        self.state.tick.0 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
