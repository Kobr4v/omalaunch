-- Omalaunch global key (append to ~/.config/hypr/bindings.lua).
-- Check current use first: `omarchy menu keybindings --print`.
-- If SUPER + A is already bound, unbind it BEFORE this line and note what
-- it was previously bound to:
--   hl.unbind("SUPER + A")  -- was: <previous action>
o.bind("SUPER + A", "Omalaunch", { launch = "omalaunch" })
