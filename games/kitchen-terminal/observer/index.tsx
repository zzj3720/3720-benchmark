import {
  Metric,
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
} from "../../../observer-platform/app/game-observer";

function KitchenState({ state }: { state: GameState }) {
  const shift = asRecord(state.shift);
  const campaign = asRecord(state.campaign);
  const orders = records(state.orders);
  const stations = records(state.stations);
  return (
    <div className="kitchen-state">
      <div className="kitchen-summary">
        <Metric label="SHIFT" value={asString(shift?.status).toUpperCase()} />
        <Metric label="ELAPSED" value={clockLabel(asNumber(shift?.elapsed_ms))} />
        <Metric label="SCORE" value={String(asNumber(campaign?.score))} />
        <Metric
          label="OPEN ORDERS"
          value={String(orders.filter((order) => order.status === "open").length)}
        />
      </div>
      <div className="kitchen-columns">
        <KitchenList
          label="ORDERS"
          empty="No active orders"
          rows={orders.map((order) => ({
            id: asString(order.id),
            primary: asString(order.title),
            secondary: `${asString(order.status).toUpperCase()} · ${clockLabel(asNumber(order.remaining_ms))}`,
          }))}
        />
        <KitchenList
          label="STATIONS"
          empty="No stations"
          rows={stations.map((station) => ({
            id: asString(station.id),
            primary: asString(station.label),
            secondary: `${asString(station.status).toUpperCase()} · ${asString(station.order, "FREE")}`,
          }))}
        />
      </div>
    </div>
  );
}

function records(value: GameState[string]) {
  return Array.isArray(value)
    ? value.map((item) => asRecord(item)).filter((item) => item !== null)
    : [];
}

function KitchenList({
  label,
  empty,
  rows,
}: {
  label: string;
  empty: string;
  rows: { id: string; primary: string; secondary: string }[];
}) {
  return (
    <section className="kitchen-list">
      <h3>{label}</h3>
      {rows.length ? (
        rows.map((row) => (
          <div key={row.id}>
            <strong>{row.primary}</strong>
            <span>{row.secondary}</span>
          </div>
        ))
      ) : (
        <p>{empty}</p>
      )}
    </section>
  );
}

function clockLabel(milliseconds: number) {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

export default {
  id: "kitchen",
  meta: { label: "Kitchen Terminal", short: "KITCHEN", accent: "#f6df5f" },
  State: KitchenState,
} satisfies GameObserverModule;
