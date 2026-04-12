import { pnlClass } from "../lib/format";

export interface CardDef {
  label: string;
  value: string;
  colorClass?: string;
  detail?: string;
}

interface StatusCardsProps {
  cards: CardDef[];
}

export function StatusCards({ cards }: StatusCardsProps) {
  return (
    <div className="cards">
      {cards.map((c) => (
        <div key={c.label} className="card">
          <div className="card-label">{c.label}</div>
          <div className={`card-value ${c.colorClass ?? (pnlClass(parseFloat(c.value)) || "")}`}>
            {c.value}
          </div>
          {c.detail && <div className="card-detail">{c.detail}</div>}
        </div>
      ))}
    </div>
  );
}
