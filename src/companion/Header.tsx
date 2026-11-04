import { Glyph } from "../ui/glyphs";
import { WidgetButton } from "./WidgetButton";

/**
 * SPEC §8.1 rule 8: [grip (drag region, flex 1, ≥28 px, no interactive
 * descendants)][menu][hide]. The drag attribute sits on the grip only.
 */
export function Header({
  label,
  menuOpen,
  onMenu,
  onHide,
}: {
  label: string;
  menuOpen: boolean;
  onMenu: () => void;
  onHide: () => void;
}) {
  return (
    <div className="cw-header">
      <span className="cw-grip" data-tauri-drag-region="deep">
        <Glyph name="grip" size={12} />
        <span className="cw-grip__label">{label}</span>
      </span>
      <WidgetButton variant="icon" icon="dots" label="Companion menu" active={menuOpen} onPress={onMenu} />
      <WidgetButton variant="icon" icon="hide" label="Hide companion" onPress={onHide} />
    </div>
  );
}
