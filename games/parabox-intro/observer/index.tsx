import {
  EmptyState,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
  type Json,
} from "../../../observer-platform/app/game-observer";

function ParaboxState({ state }: { state: GameState }) {
  const space = asRecord(state.space);
  const map = Array.isArray(space?.map) ? (space.map as Json[][]) : [];
  if (!map.length) {
    return (
      <EmptyState
        title="状态摘要已接入"
        body="该旧版 run 没有记录完整二维地图；分数、关卡和动作流仍为权威数据。"
      />
    );
  }
  return (
    <div className="parabox-state">
      <div
        className="parabox-grid"
        style={{ gridTemplateColumns: `repeat(${map[0]?.length ?? 1}, 1fr)` }}
      >
        {map.flatMap((row, rowIndex) =>
          row.map((cell, columnIndex) => {
            const symbol = asString(cell, " ");
            return (
              <span key={`${rowIndex}-${columnIndex}`} data-symbol={symbol}>
                {symbol === " " ? "" : symbol}
              </span>
            );
          }),
        )}
      </div>
    </div>
  );
}

export default {
  id: "parabox",
  meta: { label: "Patrick’s Parabox", short: "PARABOX", accent: "#a3ff62" },
  State: ParaboxState,
} satisfies GameObserverModule;
