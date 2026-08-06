import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StatusPill } from "./StatusPill";

describe("StatusPill", () => {
  it("announces a running task status", () => {
    render(<StatusPill status="running" />);
    expect(screen.getByRole("status")).toHaveTextContent("running");
  });
});
