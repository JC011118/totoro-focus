import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

// Scaffold entry; the frontend implementation replaces this.
function App() {
  return <p>Focus companion scaffold</p>;
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
