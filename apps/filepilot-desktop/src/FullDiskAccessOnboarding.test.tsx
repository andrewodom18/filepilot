import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FullDiskAccessOnboarding } from "./App";

describe("FullDiskAccessOnboarding", () => {
  it("opens the macOS privacy pane and lets the user continue", () => {
    const onOpenSettings = vi.fn().mockResolvedValue(undefined);
    const onComplete = vi.fn().mockResolvedValue(undefined);
    render(<FullDiskAccessOnboarding onOpenSettings={onOpenSettings} onComplete={onComplete} />);

    expect(screen.getByRole("dialog", { name: "See more of your Mac’s storage" })).toBeInTheDocument();
    expect(screen.getByText(/does not grant administrator access/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open Full Disk Access" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue without it" }));

    expect(onOpenSettings).toHaveBeenCalledOnce();
    expect(onComplete).toHaveBeenCalledOnce();
  });
});
