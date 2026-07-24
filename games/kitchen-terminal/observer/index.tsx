import {
  Metric,
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
  type Json,
} from "../../../observer-platform/app/game-observer";

type Point = { x: number; y: number; z: number };

const KIND_LABEL: Record<string, string> = {
  ingredient_crate: "ING",
  workstation: "CHOP",
  cooker: "HEAT",
  delivery: "SERVE",
  sink: "WASH",
  plate_return: "PLATE",
  bin: "BIN",
  switch: "SWITCH",
  loose_item: "LOOSE",
  moving_barrier: "DOOR",
  counter: "",
  structure: "",
};

function KitchenState({ state }: { state: GameState }) {
  const shift = asRecord(state.shift);
  const campaign = asRecord(state.campaign);
  const chefs = records(state.chefs);
  const orders = records(state.orders);
  const map = asRecord(state.map);
  const events = records(state.recent_events);
  const alarms = records(state.alarms);
  const hazards = records(state.hazards);
  const work = asRecord(state.work);

  return (
    <div className="kitchen-state">
      <div className="kitchen-summary">
        <Metric label="KITCHEN" value={`LEVEL ${asNumber(campaign?.level)}`} />
        <Metric label="TIME LEFT" value={clockLabel(asNumber(shift?.remaining_ms))} />
        <Metric label="SCORE" value={String(asNumber(campaign?.score))} />
        <Metric
          label="STARS"
          value={`${"★".repeat(asNumber(campaign?.stars))}${"·".repeat(3 - asNumber(campaign?.stars))}`}
        />
        <Metric label="ORDERS" value={String(orders.length)} />
      </div>

      <div className="kitchen-workspace">
        <KitchenMap map={map} chefs={chefs} work={work} hazards={hazards} />
        <OrderRail orders={orders} elapsedMs={asNumber(shift?.elapsed_ms)} />
      </div>

      <div className="kitchen-lower">
        <ChefBoard chefs={chefs} activeChef={asNumber(state.active_chef)} work={work} />
        <ActivityBoard events={events} alarms={alarms} />
      </div>
    </div>
  );
}

function KitchenMap({
  map,
  chefs,
  work,
  hazards,
}: {
  map: GameState | null;
  chefs: GameState[];
  work: GameState | null;
  hazards: GameState[];
}) {
  const cells = records(map?.walkable);
  const objects = records(map?.objects);
  const systems = records(map?.systems);
  const chefHeights = chefs
    .map((chef) => worldPoint(chef.world)?.y)
    .filter((height): height is number => height !== undefined);
  const visibleCells = cells.filter((cell) => nearActiveHeight(cell, chefHeights));
  const visibleObjects = objects.filter((object) => nearActiveHeight(object, chefHeights));
  const visibleSystems = systems.filter((system) => (
    visibleSystem(system) && nearActiveHeight(system, chefHeights)
  ));
  const points = [...visibleCells, ...visibleObjects, ...visibleSystems, ...chefs, ...hazards]
    .map((row) => worldPoint(row.world))
    .filter((point): point is Point => point !== null);
  const bounds = mapBounds(points);
  const target = asString(work?.target, "");

  return (
    <section className="kitchen-map">
      <header>
        <div>
          <span>LIVE KITCHEN FLOOR</span>
          <strong>
            {visibleCells.length === cells.length
              ? `${cells.length} WALKABLE CELLS`
              : `${visibleCells.length} ACTIVE / ${cells.length} CELLS`}
          </strong>
        </div>
        <small>REAL-TIME · TOP-DOWN</small>
      </header>
      <div className="kitchen-map-stage">
        {points.length ? (
          <svg
            aria-label="Current kitchen layout"
            preserveAspectRatio="xMidYMid meet"
            viewBox={`${bounds.x} ${bounds.z} ${bounds.width} ${bounds.height}`}
          >
            <g className="floor-cells">
              {visibleCells.map((cell, index) => {
                const point = worldPoint(cell.world);
                return point ? (
                  <rect
                    className={`floor-cell floor-${hash(asString(cell.grid_manager)) % 4}${cell.moving ? " is-moving" : ""}`}
                    height="0.94"
                    key={`${asString(cell.grid_manager)}-${index}`}
                    rx="0.08"
                    width="0.94"
                    x={point.x - 0.47}
                    y={point.z - 0.47}
                  >
                    {cell.moving ? <title>Moving platform</title> : null}
                  </rect>
                ) : null;
              })}
            </g>
            <g className="kitchen-objects">
              {visibleObjects.map((object) => (
                <KitchenObject
                  active={asString(object.id) === target}
                  key={asString(object.id)}
                  object={object}
                />
              ))}
            </g>
            <g className="kitchen-systems">
              {visibleSystems.map((system) => {
                const point = worldPoint(system.world);
                const kind = asString(system.kind);
                const targetPoint = worldPoint(
                  visibleObjects.find((object) => (
                    asString(object.id) === asString(system.target)
                  ))?.world,
                );
                const active = Boolean(system.active);
                const progress = asNumber(system.progress, 0);
                return point ? (
                  <g
                    className={`system-marker system-${kind}${active ? " is-active" : ""}`}
                    key={asString(system.id)}
                  >
                    {kind === "ConveyorStation" && targetPoint && (
                      <line
                        className="conveyor-path"
                        x1={point.x}
                        x2={targetPoint.x}
                        y1={point.z}
                        y2={targetPoint.z}
                      />
                    )}
                    <g transform={`translate(${point.x} ${point.z})`}>
                      {kind === "ConveyorStation" ? (
                        <>
                          <circle r="0.3" />
                          <circle className="conveyor-roller" cx="-0.1" r="0.04" />
                          <circle className="conveyor-roller" cx="0.1" r="0.04" />
                          {active && (
                            <circle
                              className="conveyor-progress"
                              r={0.12 + Math.min(1, progress) * 0.12}
                            />
                          )}
                        </>
                      )
                        : <path d="M 0 -0.36 L 0.36 0 L 0 0.36 L -0.36 0 Z" />}
                      <title>{`${asString(system.kind)} · ${asString(system.name)}`}</title>
                    </g>
                  </g>
                ) : null;
              })}
            </g>
            <g className="kitchen-hazards">
              {hazards.map((hazard) => (
                <HazardMarker hazard={hazard} key={asString(hazard.id)} />
              ))}
            </g>
            <g className="kitchen-chefs">
              {chefs.map((chef) => <ChefMarker chef={chef} key={asNumber(chef.id)} />)}
            </g>
          </svg>
        ) : (
          <p className="kitchen-map-empty">Waiting for a kitchen snapshot</p>
        )}
      </div>
      <footer>
        <span><i className="legend-chef" /> CHEF</span>
        <span><i className="legend-food" /> FOOD</span>
        <span><i className="legend-work" /> WORK</span>
        <span><i className="legend-system" /> MOVING / HAZARD</span>
      </footer>
    </section>
  );
}

function KitchenObject({ object, active }: { object: GameState; active: boolean }) {
  const point = worldPoint(object.world);
  if (!point) return null;
  const kind = asString(object.kind, "structure");
  const item = asRecord(object.item);
  const supply = asString(object.supply, "");
  const label = supply
    ? friendlyName(supply).slice(0, 3).toUpperCase()
    : KIND_LABEL[kind] ?? kind.slice(0, 4).toUpperCase();
  const itemLabel = itemShort(item);
  const plateCount = object.plate_count;
  const dirtyCount = object.dirty_plate_count;
  const fireStrength = typeof object.fire_strength === "number"
    ? object.fire_strength
    : null;
  const count =
    typeof plateCount === "number"
      ? plateCount
      : typeof dirtyCount === "number"
        ? dirtyCount
        : null;
  return (
    <g
      className={`kitchen-object kind-${kind}${active ? " is-working" : ""}`}
      transform={`translate(${point.x} ${point.z})`}
    >
      <rect height="1.04" rx="0.1" width="1.04" x="-0.52" y="-0.52" />
      {label && <text className="object-label" textAnchor="middle" y="0.08">{label}</text>}
      {itemLabel && (
        <g className="item-token" transform="translate(0 -0.02)">
          <circle r="0.31" />
          <text textAnchor="middle" y="0.08">{itemLabel}</text>
        </g>
      )}
      {count !== null && (
        <text className="object-count" textAnchor="middle" x="0.43" y="-0.35">{count}</text>
      )}
      {fireStrength !== null && (
        <g className="object-fire">
          <circle r={0.34 + fireStrength * 0.15} />
          <path d="M 0 .25 C -.3 .02 -.08 -.18 0 -.38 C .08 -.12 .31 .03 0 .25 Z" />
        </g>
      )}
      <title>
        {`${asString(object.name)}${supply ? ` · supplies ${supply}` : ""}${item ? ` · ${asString(item.name)}` : ""}`}
      </title>
    </g>
  );
}

function ChefMarker({ chef }: { chef: GameState }) {
  const point = worldPoint(chef.world);
  if (!point) return null;
  const active = Boolean(chef.active);
  const held = itemShort(asRecord(chef.held));
  const respawningMs = typeof chef.respawning_ms === "number"
    ? chef.respawning_ms
    : null;
  return (
    <g
      className={`chef-marker${active ? " is-active" : ""}${respawningMs !== null ? " is-respawning" : ""}`}
      transform={`translate(${point.x} ${point.z})`}
    >
      {active && <circle className="chef-pulse" r="0.68" />}
      <circle className="chef-body" r="0.43" />
      <path className="chef-hat" d="M -.3 -.34 Q -.3 -.67 0 -.53 Q .3 -.67 .3 -.34 Z" />
      <text textAnchor="middle" y="0.16">{asNumber(chef.id) + 1}</text>
      {held && (
        <g className="held-token" transform="translate(.43 -.43)">
          <circle r=".25" />
          <text textAnchor="middle" y=".07">{held}</text>
        </g>
      )}
      {respawningMs !== null && (
        <text className="respawn-label" textAnchor="middle" y="0.78">
          {Math.ceil(respawningMs / 1000)}s
        </text>
      )}
    </g>
  );
}

function HazardMarker({ hazard }: { hazard: GameState }) {
  const point = worldPoint(hazard.world);
  if (!point) return null;
  const kind = asString(hazard.kind);
  const seconds = Math.max(0, Math.ceil(asNumber(hazard.remaining_ms) / 1000));
  return (
    <g
      className={`hazard-marker hazard-${kind}`}
      transform={`translate(${point.x} ${point.z})`}
    >
      {kind === "meteor" ? (
        <>
          <circle className="meteor-zone" r="1.65" />
          <circle className="meteor-core" r=".26" />
          <text textAnchor="middle" y=".08">{seconds}</text>
        </>
      ) : kind === "fireball" ? (
        <path d="M -.42 0 L -.12 -.22 L .34 0 L -.12 .22 Z" />
      ) : (
        <circle r=".42" />
      )}
      <title>{`${friendlyName(kind)} · ${asString(hazard.id)}`}</title>
    </g>
  );
}

function OrderRail({ orders, elapsedMs }: { orders: GameState[]; elapsedMs: number }) {
  return (
    <section className="order-rail">
      <header>
        <span>ORDER RAIL</span>
        <strong>{orders.length ? "SERVICE ACTIVE" : "NO TICKETS"}</strong>
      </header>
      <div className="order-stack">
        {orders.length ? orders.map((order) => {
          const opened = asNumber(order.opened_ms);
          const deadline = asNumber(order.deadline_ms);
          const remaining = asNumber(order.remaining_ms);
          const fraction = Math.max(0, Math.min(1, remaining / Math.max(1, deadline - opened)));
          const plan = recipePlan(records(order.requirements));
          return (
            <article className={fraction < 0.25 ? "is-urgent" : ""} key={asString(order.id)}>
              <div className="ticket-pin" />
              <small>{asString(order.id).toUpperCase()}</small>
              <strong>{friendlyName(asString(order.recipe))}</strong>
              {plan && (
                <small className="recipe-plan">{plan}</small>
              )}
              <span>DUE {clockLabel(Math.max(0, deadline - elapsedMs))}</span>
              <div className="ticket-time"><i style={{ width: `${fraction * 100}%` }} /></div>
            </article>
          );
        }) : <p>Tickets appear here when the kitchen starts.</p>}
      </div>
    </section>
  );
}

function ChefBoard({
  chefs,
  activeChef,
  work,
}: {
  chefs: GameState[];
  activeChef: number;
  work: GameState | null;
}) {
  return (
    <section className="chef-board">
      <h3>CHEF CONTROL</h3>
      <div>
        {chefs.map((chef, index) => {
          const held = asRecord(chef.held);
          const respawningMs = typeof chef.respawning_ms === "number"
            ? chef.respawning_ms
            : null;
          const working = asNumber(work?.chef, -1) === asNumber(chef.id);
          const progress = working
            ? asNumber(work?.progress_ms) / Math.max(1, asNumber(work?.required_ms))
            : 0;
          return (
            <article
              className={`${index === activeChef ? "is-active" : ""}${respawningMs !== null ? " is-respawning" : ""}`}
              key={asNumber(chef.id)}
            >
              <span className="chef-index">C{asNumber(chef.id) + 1}</span>
              <div>
                <strong>
                  {respawningMs !== null
                    ? `RESPAWNING ${Math.ceil(respawningMs / 1000)}s`
                    : index === activeChef ? "ACTIVE CHEF" : "STANDBY"}
                </strong>
                <small>
                  {respawningMs !== null
                    ? "Inputs temporarily unavailable"
                    : held ? `Holding ${asString(held.name)}` : "Hands free"}
                </small>
              </div>
              {working ? (
                <div className="chef-work">
                  <span>{asString(work?.kind).toUpperCase()}</span>
                  <i style={{ width: `${Math.min(100, progress * 100)}%` }} />
                </div>
              ) : <code>{asString(chef.facing).toUpperCase()}</code>}
            </article>
          );
        })}
      </div>
    </section>
  );
}

function ActivityBoard({ events, alarms }: { events: GameState[]; alarms: GameState[] }) {
  return (
    <section className="activity-board">
      <h3>AGENT EXPERIENCE</h3>
      <div className="activity-stream">
        {events.slice(0, 6).map((event, index) => (
          <article key={`${asNumber(event.elapsed_ms)}-${index}`}>
            <time>{clockLabel(asNumber(event.elapsed_ms))}</time>
            <div>
              <strong>{friendlyName(asString(event.kind))}</strong>
              <span>{asString(event.message)}</span>
            </div>
          </article>
        ))}
        {!events.length && <p>No actions recorded yet.</p>}
      </div>
      {alarms.length > 0 && (
        <div className="alarm-strip">
          {alarms.slice(0, 3).map((alarm) => (
            <span key={asString(alarm.id)}>
              ⏱ {asString(alarm.id)} · {clockLabel(asNumber(alarm.due_ms))}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

function records(value: Json | undefined): GameState[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((item) => asRecord(item))
    .filter((item): item is GameState => item !== null);
}

function worldPoint(value: Json | undefined): Point | null {
  const point = asRecord(value);
  return point
    ? { x: asNumber(point.x), y: asNumber(point.y), z: asNumber(point.z) }
    : null;
}

function nearActiveHeight(row: GameState, chefHeights: number[]) {
  if (!chefHeights.length) return true;
  const point = worldPoint(row.world);
  return point !== null && chefHeights.some((height) => Math.abs(point.y - height) <= 2.5);
}

function recipePlan(requirements: GameState[]) {
  const nested = new Set(
    requirements.flatMap((requirement) => (
      Array.isArray(requirement.required)
        ? requirement.required.map((id) => String(id))
        : []
    )),
  );
  return requirements
    .filter((requirement) => !nested.has(asString(requirement.id)))
    .sort((left, right) => {
      const leftParts = Array.isArray(left.required) ? left.required.length : 0;
      const rightParts = Array.isArray(right.required) ? right.required.length : 0;
      return leftParts - rightParts || asString(left.id).localeCompare(asString(right.id));
    })
    .map((requirement) => {
      const ingredients = Array.isArray(requirement.required)
        ? requirement.required.map((id) => friendlyName(String(id)))
        : [];
      const result = friendlyName(asString(requirement.id));
      return ingredients.length ? `${ingredients.join("+")}→${result}` : result;
    })
    .join(" · ");
}

function mapBounds(points: Point[]) {
  const xs = points.map((point) => point.x);
  const zs = points.map((point) => point.z);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minZ = Math.min(...zs);
  const maxZ = Math.max(...zs);
  const pad = 1.2;
  return {
    x: minX - pad,
    z: minZ - pad,
    width: Math.max(4, maxX - minX + pad * 2),
    height: Math.max(4, maxZ - minZ + pad * 2),
  };
}

function itemShort(item: GameState | null) {
  if (!item) return "";
  const kind = asString(item.kind, "");
  if (kind === "plate") {
    const contents = Array.isArray(item.contents) ? item.contents : [];
    return contents.length ? String(contents.length) : "P";
  }
  if (kind === "dirty_plate_stack") return `D${asNumber(item.count)}`;
  if (kind === "extinguisher") return "EXT";
  if (kind === "container") {
    const contents = Array.isArray(item.contents) ? item.contents : [];
    return contents.length ? String(contents.length) : "POT";
  }
  return friendlyName(asString(item.name, "?")).slice(0, 2).toUpperCase();
}

function friendlyName(value: string) {
  return value
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replaceAll("_", " ")
    .replace(/\s+/g, " ")
    .trim();
}

function hash(value: string) {
  let result = 0;
  for (const char of value) result = (result * 31 + char.charCodeAt(0)) | 0;
  return Math.abs(result);
}

function visibleSystem(system: GameState) {
  return [
    "ConveyorStation",
    "FireballSpawner",
    "MeteorManager",
    "PressureSwitchCosmeticDecisions",
    "SwitchCosmeticDecisions",
    "TriggerZone",
  ].includes(asString(system.kind));
}

function clockLabel(milliseconds: number) {
  const seconds = Math.max(0, Math.ceil(milliseconds / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

export default {
  id: "kitchen",
  meta: { label: "Overcooked Kitchen", short: "KITCHEN", accent: "#ffcf54" },
  State: KitchenState,
  stateContext: (state) => {
    const campaign = asRecord(state.campaign);
    const shift = asRecord(state.shift);
    return `Kitchen level ${asNumber(campaign?.level)}, score ${asNumber(campaign?.score)}, ${clockLabel(asNumber(shift?.remaining_ms))} remaining.`;
  },
} satisfies GameObserverModule;
