import type { Column } from "../types";

interface TradesTableProps<T> {
  columns: Column<T>[];
  rows: T[];
  expandedRow?: number | null;
  onRowClick?: (index: number) => void;
  renderExpanded?: (row: T, index: number) => React.ReactNode;
  rowClassName?: (row: T, index: number) => string;
}

export function TradesTable<T>({
  columns,
  rows,
  expandedRow,
  onRowClick,
  renderExpanded,
  rowClassName,
}: TradesTableProps<T>) {
  return (
    <div className="trade-table-wrap">
      <table className="trade-table">
        <thead>
          <tr>
            {columns.map((col) => (
              <th key={col.key}>{col.header}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && (
            <tr className="empty-row">
              <td colSpan={columns.length}>No trades yet</td>
            </tr>
          )}
          {rows.map((row, i) => (
            <TradesTableRow
              key={i}
              row={row}
              index={i}
              columns={columns}
              isExpanded={expandedRow === i}
              onRowClick={onRowClick}
              renderExpanded={renderExpanded}
              rowClassName={rowClassName}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TradesTableRow<T>({
  row,
  index,
  columns,
  isExpanded,
  onRowClick,
  renderExpanded,
  rowClassName,
}: {
  row: T;
  index: number;
  columns: Column<T>[];
  isExpanded: boolean;
  onRowClick?: (index: number) => void;
  renderExpanded?: (row: T, index: number) => React.ReactNode;
  rowClassName?: (row: T, index: number) => string;
}) {
  const cls = [onRowClick ? "trade-row" : "trade-row-static", rowClassName?.(row, index) ?? ""].filter(Boolean).join(" ");
  return (
    <>
      <tr className={cls} onClick={() => onRowClick?.(index)}>
        {columns.map((col) => (
          <td key={col.key} className={col.className}>
            {col.render(row, index)}
          </td>
        ))}
      </tr>
      {isExpanded && renderExpanded && (
        <tr>
          <td colSpan={columns.length} style={{ padding: 0, borderBottom: "1px solid var(--border)" }}>
            {renderExpanded(row, index)}
          </td>
        </tr>
      )}
    </>
  );
}
