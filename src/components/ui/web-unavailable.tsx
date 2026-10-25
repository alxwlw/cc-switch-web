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
      <span className="inline-flex">{children}</span>
    </HoverTip>
  );
}
