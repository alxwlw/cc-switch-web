import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { InitErrorScreen } from "@/components/web/InitErrorScreen";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: { path?: string; detail?: string }) =>
      params?.path ? `${key}[${params.path}][${params.detail}]` : key,
  }),
}));

describe("InitErrorScreen (WEB-32)", () => {
  it("shows the title, the config path and the error detail, without any exit control", () => {
    render(
      <InitErrorScreen
        payload={{
          path: "/srv/.cc-switch/config.json",
          error: "bad json at 3",
        }}
      />,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("errors.configLoadFailedTitle");
    expect(alert).toHaveTextContent(
      "web.initError.message[/srv/.cc-switch/config.json][bad json at 3]",
    );
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("falls back to the default config path and a generic detail", () => {
    render(<InitErrorScreen payload={null} />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "web.initError.message[~/.cc-switch/config.json][Unknown error]",
    );
  });
});
