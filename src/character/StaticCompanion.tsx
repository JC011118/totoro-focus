// The built-in static companion, drawn when the active pack has no art for a
// base state (SPEC §7 "None for a base state"). An original little seedling in
// the placeholder pack's colors, with a calm expression per state, so missing
// art still looks intentional. Never animated by itself.

const C = {
  body: "#8dbb66",
  highlight: "#b4d78f",
  feet: "#6f9d4f",
  leaf: "#a9d46b",
  vein: "#6c9c43",
  stem: "#5f8d3f",
  outline: "#3b5b32",
  ink: "#2b2c26",
  blush: "#f59c9c",
};

type Eyes = "open" | "closed" | "happy" | "soft";

function eyesFor(mood: string): Eyes {
  switch (mood) {
    case "focus":
    case "sleepy":
      return "closed";
    case "completed":
    case "celebrate":
      return "happy";
    case "break":
      return "soft";
    default:
      return "open";
  }
}

function EyePair({ eyes }: { eyes: Eyes }) {
  const stroke = { stroke: C.ink, strokeWidth: 2, strokeLinecap: "round" as const, fill: "none" };
  switch (eyes) {
    case "closed":
      return (
        <g {...stroke}>
          <path d="M35 60.5q4 3 8 0" />
          <path d="M53 60.5q4 3 8 0" />
        </g>
      );
    case "happy":
      return (
        <g {...stroke}>
          <path d="M35 61.5q4-4.5 8 0" />
          <path d="M53 61.5q4-4.5 8 0" />
        </g>
      );
    case "soft":
      return (
        <g {...stroke}>
          <path d="M35 60q4 1.6 8 0" />
          <path d="M53 60q4 1.6 8 0" />
        </g>
      );
    case "open":
      return (
        <g>
          <circle cx="39" cy="60" r="2.8" fill={C.ink} />
          <circle cx="57" cy="60" r="2.8" fill={C.ink} />
          <circle cx="40" cy="59" r="0.9" fill="#fff" />
          <circle cx="58" cy="59" r="0.9" fill="#fff" />
        </g>
      );
  }
}

export function StaticCompanion({ mood, size }: { mood: string; size: number }) {
  const eyes = eyesFor(mood);
  const celebrating = eyes === "happy";
  return (
    <svg
      className="static-companion"
      data-mood={mood}
      width={size}
      height={size}
      viewBox="0 0 96 96"
      aria-hidden="true"
      focusable="false"
    >
      <ellipse cx="48" cy="88.5" rx="21" ry="3.2" fill={C.outline} opacity="0.16" />
      {/* feet */}
      <ellipse cx="38.5" cy="84" rx="7" ry="4" fill={C.feet} stroke={C.outline} strokeWidth="1.8" />
      <ellipse cx="57.5" cy="84" rx="7" ry="4" fill={C.feet} stroke={C.outline} strokeWidth="1.8" />
      {/* stem and leaves */}
      <path d="M48 42C47.5 36 48.5 31 50 27" fill="none" stroke={C.stem} strokeWidth="3" strokeLinecap="round" />
      <path
        d="M49.5 28C44 19 35 17.5 28.5 20.5C31 28.5 40 32 49.5 28Z"
        fill={C.leaf}
        stroke={C.outline}
        strokeWidth="1.7"
        strokeLinejoin="round"
      />
      <path d="M47 27.5C41 24.5 36 23 31.5 22" fill="none" stroke={C.vein} strokeWidth="1.2" strokeLinecap="round" />
      <path
        d="M50.5 27.5C55 17 64 14 71 16.5C69.5 25 60.5 30 50.5 27.5Z"
        fill={C.leaf}
        stroke={C.outline}
        strokeWidth="1.7"
        strokeLinejoin="round"
      />
      <path d="M53 26C58.5 22 63.5 19.5 68 18.5" fill="none" stroke={C.vein} strokeWidth="1.2" strokeLinecap="round" />
      {/* arms */}
      <path d="M24.5 64.5q-4 3-2.5 7" fill="none" stroke={C.outline} strokeWidth="2" strokeLinecap="round" />
      <path d="M71.5 64.5q4 3 2.5 7" fill="none" stroke={C.outline} strokeWidth="2" strokeLinecap="round" />
      {/* body */}
      <path
        d="M48 39C63 39 74 50 74 63.5C74 76 63 84.5 48 84.5C33 84.5 22 76 22 63.5C22 50 33 39 48 39Z"
        fill={C.body}
        stroke={C.outline}
        strokeWidth="2.2"
      />
      <ellipse cx="39" cy="52" rx="10" ry="6.5" fill={C.highlight} opacity="0.7" />
      <circle cx="61" cy="74" r="1.3" fill={C.feet} />
      <circle cx="65" cy="70" r="1" fill={C.feet} />
      <circle cx="31" cy="74.5" r="1.1" fill={C.feet} />
      <EyePair eyes={eyes} />
      <ellipse cx="32.5" cy="67" rx="4" ry="2.4" fill={C.blush} opacity="0.6" />
      <ellipse cx="63.5" cy="67" rx="4" ry="2.4" fill={C.blush} opacity="0.6" />
      <path
        d={celebrating ? "M44 66.5q4 4.5 8 0" : "M45 67q3 2.4 6 0"}
        fill="none"
        stroke={C.ink}
        strokeWidth="1.7"
        strokeLinecap="round"
      />
      {celebrating && (
        <g fill={C.leaf} stroke={C.outline} strokeWidth="1">
          <path d="M16 40l1.6 3.4 3.4 1.6-3.4 1.6L16 50l-1.6-3.4L11 45l3.4-1.6Z" />
          <path d="M80 34l1.2 2.6 2.6 1.2-2.6 1.2L80 41.6l-1.2-2.6-2.6-1.2 2.6-1.2Z" />
        </g>
      )}
    </svg>
  );
}
