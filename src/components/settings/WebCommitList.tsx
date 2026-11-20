import { useTranslation } from "react-i18next";
import type { WebUpdateCheck } from "@/lib/updater";

/**
 * The commits the branch has that this build lacks, newest first. Shared by the About block and
 * the self-update confirmation, so the list is one piece of markup.
 */
export function WebCommitList({
  commits,
  behindBy,
}: Pick<WebUpdateCheck, "commits" | "behindBy">) {
  const { t } = useTranslation();
  if (commits.length === 0) return null;
  return (
    <div className="space-y-1">
      <p className="text-caption font-medium text-fg-2">
        {t("web.update.commitsTitle")}
        {commits.length < behindBy && (
          <>
            {" · "}
            {t("web.update.shown", {
              shown: commits.length,
              total: behindBy,
            })}
          </>
        )}
      </p>
      <ul className="space-y-0.5">
        {commits.map((commit) => (
          <li key={commit.sha} className="flex gap-2 text-fg-2">
            <code className="shrink-0 tabular-nums text-fg-1">
              {commit.sha.slice(0, 7)}
            </code>
            <span className="min-w-0 break-words">{commit.message}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
