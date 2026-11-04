import { fireEvent, render, screen, within } from "@testing-library/react";
import i18next from "i18next";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import { SelfUpdateButton } from "@/components/settings/SelfUpdatePanel";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";
import type { WebUpdateCheck } from "@/lib/updater";

// The real catalogs and the real i18next: nothing here mocks `t`, so a key whose text was
// replaced (or lost its placeholder) shows up. Component tests that mock react-i18next return
// the key and the options and cannot see either.
const catalogs = { en, ja, tr, "zh-TW": zhTW, zh } as const;
type Catalog = { [key: string]: unknown };

function lookup(catalog: Catalog, path: string): unknown {
  return path
    .split(".")
    .reduce<unknown>(
      (node, key) => (node as Catalog | undefined)?.[key],
      catalog,
    );
}

// `settings.backupManager.confirmMessage` is the database restore confirmation
// (BackupListSection). Its text as it was at 56b95d1a, before WEB-28 touched the catalogs.
const RESTORE_CONFIRM = {
  en: "Restoring this backup will overwrite the current database. A safety backup will be created first.",
  ja: "このバックアップを復元すると現在のデータベースが上書きされます。安全バックアップが先に作成されます。",
  tr: "Bu yedeğin geri yüklenmesi mevcut veritabanının üzerine yazılacaktır. İlk önce bir güvenlik yedeği oluşturulacaktır.",
  "zh-TW": "還原至此備份將覆寫目前資料庫。還原前會自動建立安全備份。",
  zh: "恢复到此备份将覆盖当前数据库。恢复前会自动创建安全备份。",
} as const;

describe("catalogs · self-update strings (WEB-28)", () => {
  it.each(Object.keys(catalogs) as (keyof typeof catalogs)[])(
    "%s: the database restore confirmation still says the database is overwritten",
    (lang) => {
      expect(
        lookup(catalogs[lang], "settings.backupManager.confirmMessage"),
      ).toBe(RESTORE_CONFIRM[lang]);
    },
  );

  it.each(Object.keys(catalogs) as (keyof typeof catalogs)[])(
    "%s: the self-update confirmation names the branch, the target and the count",
    async (lang) => {
      const text = lookup(
        catalogs[lang],
        "web.update.install.confirmMessage",
      ) as string;
      expect(text).toContain("{{branch}}");
      expect(text).toContain("{{sha}}");
      expect(text).toContain("{{behind}}");

      const i18n = i18next.createInstance();
      await i18n.init({
        lng: lang,
        resources: { [lang]: { translation: catalogs[lang] } },
        interpolation: { escapeValue: false },
      });
      const shown = i18n.t("web.update.install.confirmMessage", {
        branch: "web-parity",
        sha: "fedcba9",
        behind: 3,
      });
      expect(shown).toContain("web-parity");
      expect(shown).toContain("fedcba9");
      expect(shown).not.toContain("{{");
    },
  );
});

describe("self-update confirmation · real catalog render", () => {
  const webUpdate: WebUpdateCheck = {
    status: "behind",
    currentVersion: "4.0.20",
    currentSha: "0123456789abcdef0123456789abcdef01234567",
    latestSha: "fedcba9876543210fedcba9876543210fedcba98",
    behindBy: 2,
    commits: [
      {
        sha: "fedcba9876543210fedcba9876543210fedcba98",
        message: "feat: newest",
        date: null,
      },
      { sha: "bbbbbbb2222222", message: "fix: middle", date: null },
    ],
    checkedAt: "2026-11-04T10:30:00Z",
    branch: "web-parity",
    instructions: { systemd: "s", docker: "d" },
  };

  it("the dialog shows the branch name, not a placeholder", async () => {
    const i18n = i18next.createInstance();
    await i18n.init({
      lng: "en",
      resources: { en: { translation: en } },
      interpolation: { escapeValue: false },
    });
    render(
      <I18nextProvider i18n={i18n}>
        <SelfUpdateButton
          webUpdate={webUpdate}
          state={{ kind: "idle" }}
          onStart={async () => {}}
        />
      </I18nextProvider>,
    );
    fireEvent.click(screen.getByText("Update now"));
    const dialog = screen.getByRole("dialog");
    const message = within(dialog).getByText(/will pull the branch/);
    expect(message).toHaveTextContent(
      "The server will pull the branch web-parity (up to fedcba9, 2 new commit(s))",
    );
    expect(dialog).not.toHaveTextContent("{{");
    expect(
      within(dialog).getByText(/may migrate the database/),
    ).toBeInTheDocument();
  });
});
