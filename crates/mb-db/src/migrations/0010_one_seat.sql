-- 0010 — one phone, one seat. A phone names its install when it pairs; the counter keeps ONE row
-- per install, so a phone that signs out and comes back takes its own seat again instead of
-- filling the plan with copies of itself. Old rows have no install and stay as they are.
ALTER TABLE lan_devices ADD COLUMN install_id TEXT;
CREATE INDEX idx_lan_devices_install ON lan_devices (outlet_id, install_id) WHERE install_id IS NOT NULL;
