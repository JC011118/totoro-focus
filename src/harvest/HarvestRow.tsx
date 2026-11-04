// Today's harvest (SPEC §6): one acorn per completed session, one leaf per
// session that ended early, in history order; the first `max`, then "+N".

import type { HarvestItem } from "../ipc/bindings/HarvestItem";
import { capHarvest, harvestLabel, HARVEST_MAX } from "../format/harvest";
import { AcornIcon, LeafIcon } from "./icons";

export interface HarvestRowProps {
  items: readonly HarvestItem[];
  max?: number;
  iconSize?: number;
  /** Shown (muted) when there is nothing yet. */
  emptyText?: string;
}

export function HarvestRow({ items, max = HARVEST_MAX, iconSize = 13, emptyText }: HarvestRowProps) {
  const { shown, overflow } = capHarvest(items, max);
  return (
    <div className="harvest" role="img" aria-label={harvestLabel(items)}>
      {shown.length === 0 && emptyText ? <span className="harvest__empty">{emptyText}</span> : null}
      {shown.map((item, i) => (
        <span key={i} className={`harvest__item harvest__item--${item}`}>
          {item === "acorn" ? <AcornIcon size={iconSize} /> : <LeafIcon size={iconSize} />}
        </span>
      ))}
      {overflow > 0 ? <span className="harvest__more">+{overflow}</span> : null}
    </div>
  );
}
