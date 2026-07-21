import {
  Metric,
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
} from "../../../observer-platform/app/game-observer";

function OperatorState({ state }: { state: GameState }) {
  const shift = asRecord(state.shift);
  const calls = records(state.calls);
  const incidents = records(state.incidents);
  const units = records(state.units);
  const alarms = records(state.alarms);
  const activeCalls = calls.filter((call) => ["ringing", "active"].includes(asString(call.status)));
  const openIncidents = incidents.filter(
    (incident) => !["resolved", "lost"].includes(asString(incident.status)),
  );
  return (
    <div className="operator-state">
      <div className="operator-summary">
        <Metric label="SHIFT" value={asString(shift?.status).toUpperCase()} />
        <Metric label="ELAPSED" value={clockLabel(asNumber(shift?.elapsed_ms))} />
        <Metric label="CALLS" value={String(activeCalls.length)} />
        <Metric label="INCIDENTS" value={String(openIncidents.length)} />
      </div>
      <div className="operator-columns">
        <OperatorList
          label="CALL QUEUE"
          empty="No active calls"
          rows={activeCalls.map((call) => ({
            id: asString(call.id),
            primary: asString(call.caller),
            secondary: asString(call.status).toUpperCase(),
          }))}
        />
        <OperatorList
          label="INCIDENTS"
          empty="No reported incidents"
          rows={incidents.map((incident) => ({
            id: asString(incident.id),
            primary: asString(incident.title),
            secondary: `${asString(incident.status).toUpperCase()} · ${Math.max(0, asNumber(incident.health_milli) / 1000).toFixed(0)}%`,
          }))}
        />
        <OperatorList
          label="UNITS"
          empty="No units"
          rows={units.map((unit) => ({
            id: asString(unit.id),
            primary: asString(unit.label),
            secondary: `${asString(unit.role).toUpperCase()} · ${asString(unit.status).toUpperCase()}`,
          }))}
        />
        <OperatorList
          label="ALARMS"
          empty="No pending alarms"
          rows={alarms
            .filter((alarm) => ["pending", "due"].includes(asString(alarm.status)))
            .map((alarm) => ({
              id: asString(alarm.id),
              primary: asString(alarm.note, "Reminder"),
              secondary: `${asString(alarm.status).toUpperCase()} · ${clockLabel(asNumber(alarm.due_ms))}`,
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

function OperatorList({
  label,
  empty,
  rows,
}: {
  label: string;
  empty: string;
  rows: { id: string; primary: string; secondary: string }[];
}) {
  return (
    <section className="operator-list">
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
  id: "operator",
  meta: {
    label: "Emergency Operator",
    short: "OPERATOR",
    accent: "#ff6f7d",
  },
  State: OperatorState,
} satisfies GameObserverModule;
