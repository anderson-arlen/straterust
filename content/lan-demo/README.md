# Original two-player LAN demonstration

An original symmetrical map with two human players, fog, workers, bases,
rifle units and mineral fields. Rules and original terrain derive from the
repository's Terran demonstration; no proprietary art, audio or archive data
is included. This content uses the repository's MIT license.

Host with `straterust-client --package content/lan-demo --host 0.0.0.0:6112`.
Join with `--package content/lan-demo --join HOST_IP:6112`. Compatible installed
rules are required; the host supplies the public map. Gather, construct supply
and production buildings, train an army and destroy the other player's units.
The shared ridge has a central opening. Each player starts with 50 minerals.

Space pauses/resumes for the host. LAN game menus do not pause the match.
Headless hosting, joining, discovery and replay verification are available through
`straterust-session --help`.
