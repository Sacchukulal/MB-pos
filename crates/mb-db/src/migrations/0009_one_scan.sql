-- 0009 — one scan. A phone is let in at the counter: whoever manages phones picks the person
-- from the staff list and presses Allow. Nothing is typed on the phone, so the two columns a
-- phone used to type are gone: the staff code, and the owner's "can log in on a phone" switch
-- (Allow IS that switch). The counter's own lock screen picks a person by tapping a name.
ALTER TABLE staff DROP COLUMN code;
ALTER TABLE staff DROP COLUMN can_login_on_phone;
