import {
  Metric,
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
  type Json,
} from "../../../observer-platform/app/game-observer";

export function OperatorState({ state }: { state: GameState }) {
  const campaign = asRecord(state.campaign);
  const shift = asRecord(state.shift);
  const duty = asRecord(shift?.duty);
  const calls = records(state.calls);
  const incidents = records(state.incidents);
  const units = records(state.units);
  const alarms = records(state.alarms);
  const activeCalls = calls.filter((call) => ["ringing", "active"].includes(asString(call.status)));
  const openIncidents = incidents.filter((incident) => asString(incident.status) === "reported");
  const focusCall =
    activeCalls.find((call) => asString(call.status) === "active") ?? activeCalls[0] ?? null;

  return (
    <div className="operator-state">
      <div className="operator-summary">
        <Metric
          label="DUTY"
          value={
            duty
              ? `C${asNumber(duty.chapter)} · D${asNumber(duty.number)}`
              : asString(shift?.status).toUpperCase()
          }
        />
        <Metric label="CITY" value={asString(duty?.city).toUpperCase()} />
        <Metric label="CLOCK" value={clockLabel(asNumber(shift?.elapsed_ms))} />
        <Metric
          label="SCORE"
          value={`${asNumber(campaign?.score).toLocaleString()} / ${asNumber(campaign?.max_score).toLocaleString()}`}
        />
        <Metric label="LIVE" value={`${activeCalls.length} CALL · ${openIncidents.length} SCENE`} />
      </div>

      <div className="operator-workspace">
        <OperatorMap incidents={incidents} units={units} city={asString(duty?.city, "Dispatch")} />
        <CallConsole call={focusCall} />
      </div>

      <div className="operator-detail-grid">
        <IncidentBoard incidents={incidents} />
        <UnitBoard units={units} />
        <ExperienceBoard calls={calls} alarms={alarms} />
      </div>
    </div>
  );
}

function OperatorMap({
  incidents,
  units,
  city,
}: {
  incidents: GameState[];
  units: GameState[];
  city: string;
}) {
  const visibleIncidents = incidents.filter(
    (incident) => asString(incident.status) === "reported",
  );
  return (
    <section className="operator-map">
      <header>
        <div>
          <span>LIVE DISPATCH MAP</span>
          <strong>{city}</strong>
        </div>
        <small>SECTOR GRID · INCIDENTS & UNIT ASSIGNMENTS</small>
      </header>
      <div className="operator-map-grid">
        <div className="map-axis map-axis-x">
          <span>−8</span><span>−4</span><span>0</span><span>+4</span><span>+8</span>
        </div>
        <div className="map-axis map-axis-y">
          <span>+8</span><span>+4</span><span>0</span><span>−4</span><span>−8</span>
        </div>
        {visibleIncidents.map((incident) => {
          const point = asRecord(incident.location);
          const role = incidentRole(incident);
          return (
            <div
              className={`map-pin incident-pin role-${role}`}
              key={asString(incident.id)}
              style={mapPosition(point)}
              title={asString(incident.title)}
            >
              <span>{role.slice(0, 1).toUpperCase()}</span>
              <strong>{shortLabel(asString(incident.title))}</strong>
            </div>
          );
        })}
        {units.map((unit, index) => {
          const point = asRecord(unit.location);
          return (
            <div
              className={`map-pin unit-pin role-${asString(unit.role)}`}
              key={asString(unit.id)}
              style={mapPosition(point, index % 2 ? 0.22 : -0.22)}
              title={`${asString(unit.label)} · ${asString(unit.status)}`}
            >
              {asString(unit.id).replace(/[^0-9]/g, "") || "•"}
            </div>
          );
        })}
      </div>
      <footer>
        <span><i className="role-police" /> POLICE</span>
        <span><i className="role-medical" /> MEDICAL</span>
        <span><i className="role-fire" /> FIRE</span>
      </footer>
    </section>
  );
}

function CallConsole({ call }: { call: GameState | null }) {
  if (!call) {
    return (
      <section className="call-console call-console-empty">
        <span>CALL CONSOLE</span>
        <strong>LINE CLEAR</strong>
        <p>Waiting for the next caller. Reports may still require dispatch.</p>
      </section>
    );
  }
  const transcript = records(call.transcript);
  const choices = records(call.choices);
  return (
    <section className="call-console">
      <header>
        <div>
          <span>{asString(call.status).toUpperCase()}</span>
          <strong>{asString(call.caller)}</strong>
        </div>
        <small>
          {call.answer_deadline_ms
            ? `ANSWER BY ${clockLabel(asNumber(call.answer_deadline_ms))}`
            : `END BY ${clockLabel(asNumber(call.conversation_deadline_ms))}`}
        </small>
      </header>
      <div className="transcript">
        {transcript.length ? (
          transcript.slice(-7).map((line, index) => (
            <p className={`speaker-${asString(line.speaker)}`} key={`${index}-${asString(line.text)}`}>
              <span>{asString(line.speaker).toUpperCase()}</span>
              {asString(line.text)}
            </p>
          ))
        ) : (
          <p className="ringing-copy">Incoming line. The Agent must answer it explicitly.</p>
        )}
      </div>
      {choices.length > 0 && (
        <div className="call-choices">
          <span>AVAILABLE RESPONSES</span>
          {choices.map((choice) => (
            <div key={asString(choice.id)}>
              <code>{asString(choice.id)}</code>
              <strong>{asString(choice.text)}</strong>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function IncidentBoard({ incidents }: { incidents: GameState[] }) {
  const visible = incidents
    .filter((incident) => asString(incident.status) !== "hidden")
    .slice(-8)
    .reverse();
  return (
    <section className="operator-board incident-board">
      <h3>INCIDENT SCENES</h3>
      {visible.length ? (
        visible.map((incident) => {
          const elements = records(incident.elements).filter((element) => element.active);
          const requirements = records(incident.requirements);
          return (
            <article key={asString(incident.id)}>
              <header>
                <strong>{asString(incident.title)}</strong>
                <span className={`status-${asString(incident.status)}`}>
                  {asString(incident.status).toUpperCase()}
                </span>
              </header>
              <div className="incident-progress">
                {requirements.map((requirement) => {
                  const total = Math.max(1, asNumber(requirement.total_work_ms));
                  const remaining = asNumber(requirement.remaining_work_ms);
                  return (
                    <span key={asString(requirement.role)}>
                      <i
                        className={`role-${asString(requirement.role)}`}
                        style={{ width: `${Math.max(3, (1 - remaining / total) * 100)}%` }}
                      />
                      {asString(requirement.role).toUpperCase()} {clockLabel(remaining)}
                    </span>
                  );
                })}
              </div>
              <div className="scene-elements">
                {elements.slice(0, 6).map((element) => (
                  <span key={asString(element.id)}>
                    {asString(element.kind).toUpperCase()} · {asString(element.label)}
                    {element.health_milli !== null
                      ? ` · ${Math.max(0, asNumber(element.health_milli) / 1000).toFixed(0)}%`
                      : ""}
                    {element.remaining_timer_ms !== null &&
                    element.remaining_timer_ms !== undefined
                      ? ` · T−${clockLabel(asNumber(element.remaining_timer_ms))}`
                      : ""}
                    {element.bill !== null && element.bill !== undefined
                      ? ` · BILL ${asNumber(element.bill).toLocaleString()}`
                      : ""}
                  </span>
                ))}
              </div>
            </article>
          );
        })
      ) : (
        <p>No reported incidents</p>
      )}
    </section>
  );
}

function UnitBoard({ units }: { units: GameState[] }) {
  return (
    <section className="operator-board unit-board">
      <h3>RESPONSE UNITS</h3>
      {units.map((unit) => (
        <article key={asString(unit.id)}>
          <i className={`role-${asString(unit.role)}`} />
          <div>
            <strong>{asString(unit.label)}</strong>
            <span>{asString(unit.incident, "Dispatch base")}</span>
          </div>
          <small>
            {asString(unit.status).toUpperCase()}
            {unit.eta_ms !== null ? ` · ${clockLabel(asNumber(unit.eta_ms))}` : ""}
          </small>
        </article>
      ))}
    </section>
  );
}

function ExperienceBoard({ calls, alarms }: { calls: GameState[]; alarms: GameState[] }) {
  const reports = calls
    .flatMap((call) =>
      strings(call.after_action_report).map((text) => ({ id: asString(call.id), text })),
    )
    .slice(-6)
    .reverse();
  const pendingAlarms = alarms.filter((alarm) =>
    ["pending", "due"].includes(asString(alarm.status)),
  );
  const facts = calls
    .flatMap((call) =>
      Object.entries(asRecord(call.facts) ?? {}).map(([key, value]) => ({
        id: asString(call.id),
        key,
        value: asString(value),
      })),
    )
    .slice(-6)
    .reverse();
  return (
    <section className="operator-board experience-board">
      <h3>AGENT MEMORY & AAR</h3>
      {reports.length ? (
        reports.map((report, index) => (
          <article key={`${report.id}-${index}`}>
            <span>AAR</span>
            <p>{report.text}</p>
          </article>
        ))
      ) : (
        <p>No accumulated after-action findings yet</p>
      )}
      {facts.map((fact) => (
        <article key={`${fact.id}-${fact.key}`}>
          <span>FACT · {fact.key.toUpperCase()}</span>
          <p>{fact.value}</p>
        </article>
      ))}
      {pendingAlarms.map((alarm) => (
        <article className="memory-alarm" key={asString(alarm.id)}>
          <span>ALARM {clockLabel(asNumber(alarm.due_ms))}</span>
          <p>{asString(alarm.note, "Agent reminder")}</p>
        </article>
      ))}
    </section>
  );
}

function records(value: Json | undefined) {
  return Array.isArray(value)
    ? value.map((item) => asRecord(item)).filter((item): item is GameState => item !== null)
    : [];
}

function strings(value: Json | undefined) {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function mapPosition(point: GameState | null, xOffset = 0) {
  const x = Math.max(-8, Math.min(8, asNumber(point?.x) + xOffset));
  const y = Math.max(-8, Math.min(8, asNumber(point?.y)));
  return { left: `${((x + 8) / 16) * 88 + 6}%`, top: `${((8 - y) / 16) * 82 + 9}%` };
}

function incidentRole(incident: GameState) {
  const requirements = records(incident.requirements);
  return asString(requirements[0]?.role, "police");
}

function shortLabel(value: string) {
  return value.length > 18 ? `${value.slice(0, 16)}…` : value;
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
