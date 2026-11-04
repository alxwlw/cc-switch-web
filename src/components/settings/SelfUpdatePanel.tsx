import { useState, type ReactNode } from "react";
import { AlertTriangle, Loader2, Download } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { WebCommitList } from "@/components/settings/WebCommitList";
import {
  SELF_UPDATE_JOURNAL_COMMAND,
  type SelfUpdateState,
} from "@/hooks/useSelfUpdate";
import type { WebUpdateCheck } from "@/lib/updater";

/** Stages `scripts/self-update.sh` writes while it works; others are shown by their own name. */
const PROGRESS_STAGES = [
  "checking",
  "fetching",
  "merging",
  "building",
  "installing",
  "restarting",
  "rolling-back",
] as const;

/** The button is offered unless a run is under way or a rollback left the service in doubt. */
export function canStartSelfUpdate(state: SelfUpdateState): boolean {
  switch (state.kind) {
    case "starting":
    case "running":
    case "reloading":
      return false;
    case "ended":
      return state.outcome !== "rollback-failed";
    default:
      return true;
  }
}

/** "Update now" with its confirmation; shown in the behind block of the About section. */
export function SelfUpdateButton({
  webUpdate,
  state,
  onStart,
}: {
  webUpdate: WebUpdateCheck;
  state: SelfUpdateState;
  onStart: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  if (!canStartSelfUpdate(state)) return null;

  return (
    <>
      <div>
        <Button
          type="button"
          variant="solid"
          size="regular"
          onClick={() => setConfirming(true)}
        >
          <Download className="h-3.5 w-3.5" />
          {t("web.update.install.button")}
        </Button>
      </div>
      <ConfirmDialog
        isOpen={confirming}
        variant="info"
        title={t("web.update.install.confirmTitle")}
        message={t("web.update.install.confirmMessage", {
          sha: (webUpdate.latestSha ?? "").slice(0, 7),
          behind: webUpdate.behindBy,
        })}
        confirmText={t("web.update.install.confirmButton")}
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          void onStart();
        }}
      >
        <div className="space-y-3 text-sm">
          <WebCommitList
            commits={webUpdate.commits}
            behindBy={webUpdate.behindBy}
          />
          <p className="flex gap-2 text-fg-1">
            <AlertTriangle
              aria-hidden="true"
              className="mt-0.5 h-4 w-4 shrink-0 text-destructive"
            />
            <span className="min-w-0 break-words">
              {t("web.update.install.confirmDbWarning")}
            </span>
          </p>
        </div>
      </ConfirmDialog>
    </>
  );
}

function Problem({
  title,
  message,
  children,
  strong = false,
}: {
  title: string;
  message?: string;
  children?: ReactNode;
  strong?: boolean;
}) {
  return (
    <div role="alert" className="space-y-1 px-5 py-4 text-body">
      <p
        className={
          strong ? "font-medium text-destructive" : "font-medium text-fg-1"
        }
      >
        {title}
      </p>
      {message ? <p className="break-words text-fg-2">{message}</p> : null}
      {children}
    </div>
  );
}

/** Progress and outcome of the run, under the About update block. Idle: nothing. */
export function SelfUpdateNotice({ state }: { state: SelfUpdateState }) {
  const { t } = useTranslation();

  switch (state.kind) {
    case "idle":
      return null;
    case "starting":
    case "running":
    case "reloading": {
      const stage = state.kind === "running" ? state.stage : null;
      const label =
        state.kind === "reloading"
          ? t("web.update.install.reloading")
          : stage === null
            ? t("web.update.install.starting")
            : (PROGRESS_STAGES as readonly string[]).includes(stage)
              ? t(`web.update.install.stage.${stage}`)
              : stage;
      return (
        <div
          role="status"
          aria-live="polite"
          className="space-y-1 px-5 py-4 text-body"
        >
          <p className="flex items-center gap-2 font-medium text-fg-1">
            <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
            <span className="min-w-0 break-words">
              {t("web.update.install.runningTitle")}
            </span>
          </p>
          <p className="break-words text-fg-2">{label}</p>
          {state.kind === "running" && state.branch ? (
            <p className="break-words text-caption text-fg-2">
              {t("web.update.install.branch", { branch: state.branch })}
            </p>
          ) : null}
          {state.kind === "running" && state.waiting ? (
            <p className="text-caption text-fg-2">
              {t("web.update.install.waiting")}
            </p>
          ) : null}
        </div>
      );
    }
    case "refused":
      return (
        <Problem
          title={t("web.update.install.refusedTitle")}
          message={state.message}
        />
      );
    case "stuck":
      return (
        <Problem
          title={t("web.update.install.stuckTitle")}
          message={t(
            state.reason === "stalled"
              ? "web.update.install.stuckStalled"
              : "web.update.install.stuckNoStatus",
          )}
        >
          <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-control bg-subtle p-2 text-caption text-fg-1">
            {SELF_UPDATE_JOURNAL_COMMAND}
          </pre>
        </Problem>
      );
    case "ended":
      switch (state.outcome) {
        case "unchanged":
          return (
            <div role="status" className="space-y-1 px-5 py-4 text-body">
              <p className="font-medium text-fg-1">
                {t("web.update.install.unchangedTitle")}
              </p>
              {state.message ? (
                <p className="break-words text-fg-2">{state.message}</p>
              ) : null}
            </div>
          );
        case "rollback-failed":
          return (
            <Problem
              strong
              title={t("web.update.install.rollbackFailedTitle")}
              message={state.message}
            >
              <p className="text-fg-1">
                {t("web.update.install.rollbackFailedHint")}
              </p>
            </Problem>
          );
        case "rolled-back":
          return (
            <Problem
              title={t("web.update.install.rolledBackTitle")}
              message={state.message}
            />
          );
        default:
          return (
            <Problem
              title={t("web.update.install.failedTitle")}
              message={state.message}
            />
          );
      }
  }
}
