import * as React from "react";
import { useTranslation } from "react-i18next";
import { HoverTip } from "@/components/ui/hover-tip";

/**
 * WEB-00: при `active` (вызывающий передаёт `isWeb`, прочитанный при рендере) оборачивает
 * уже disabled контрол в подсказку `web.unavailable`. disabled-кнопка не получает
 * pointer-события, поэтому подсказка вешается на обёртку. Без `active` — children как есть.
 */
export function WebUnavailableTip({
  active,
  children,
}: {
  active: boolean;
  children: React.ReactElement;
}) {
  const { t } = useTranslation();
  if (!active) return children;
  return (
    <HoverTip content={t("web.unavailable")}>
      <span className="inline-flex [&>*]:pointer-events-none">{children}</span>
    </HoverTip>
  );
}

/**
 * Вход в аккаунт Kimi (OAuth) недоступен в web до WEB-02 этапа 2 (фаза 26) — единственный
 * пользователь: Центр авторизации. Copilot/Codex/xAI открыты WEB-02 этапа 1.
 */
export function WebAccountLoginNotice({ className }: { className?: string }) {
  const { t } = useTranslation();
  return (
    <p role="status" className={className ?? "text-body text-fg-2"}>
      {t("web.accountLogin")}
    </p>
  );
}
