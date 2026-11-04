// Harvest icons: an acorn for a completed session, a small leaf for one that
// ended early. Both are friendly; the leaf is never a "failure" mark.

export function AcornIcon({ size = 14, title }: { size?: number; title?: string }) {
  return (
    <svg
      className="icon icon--acorn"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      focusable="false"
    >
      <path d="M8 1.6v1.6" stroke="#5b3d22" strokeWidth="1.4" strokeLinecap="round" />
      <path d="M3.2 6.6C3.2 9.8 5.3 13.6 8 14.4C10.7 13.6 12.8 9.8 12.8 6.6Z" fill="#c98a4b" stroke="#6b4423" strokeWidth="1" strokeLinejoin="round" />
      <path d="M5 8.2c.3 2 1.3 3.9 2.6 4.8" fill="none" stroke="#e2b07a" strokeWidth="1" strokeLinecap="round" />
      <path d="M2.4 6.8C2.4 4.2 5 3 8 3s5.6 1.2 5.6 3.8Z" fill="#8a5a31" stroke="#5b3d22" strokeWidth="1" strokeLinejoin="round" />
      <path d="M4.6 4.9h1.2M7.4 4.4h1.2M10.2 4.9h1.2" stroke="#b07a46" strokeWidth=".9" strokeLinecap="round" />
    </svg>
  );
}

export function LeafIcon({ size = 14, title }: { size?: number; title?: string }) {
  return (
    <svg
      className="icon icon--leaf"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      focusable="false"
    >
      <path d="M3 13.2C2.6 7.8 6.2 3.2 13.2 2.8C13.6 9.4 9.6 13.4 3 13.2Z" fill="#9cc56f" stroke="#4f7a35" strokeWidth="1" strokeLinejoin="round" />
      <path d="M3.6 12.6C6.2 10 8.6 7.6 11.4 4.8" fill="none" stroke="#4f7a35" strokeWidth=".9" strokeLinecap="round" />
    </svg>
  );
}
