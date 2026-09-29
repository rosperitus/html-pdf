# PLAN

## Поддерживаемое подмножество (v1)

- HTML: `html body div p span h1-h6 b strong i em u br hr ul ol li table tr td th img a`
  плюс любые неизвестные теги как inline/block по `display`. Сущности `&amp;` и `&#NNN;`.
- CSS: `<style>` и `style=""`. Селекторы: тег, `.class`, `#id`, потомок, группы через запятую.
  Специфичность и наследование по CSS 2.1.
- Свойства: `display` (block/inline/none/list-item/table*), `margin`, `padding`, `border`
  (сплошная), `width`, `color`, `background-color`, `font-family/size/weight/style`,
  `line-height`, `text-align`, `text-decoration: underline`,
  `page-break-before/after/inside`, `break-*`, `orphans`, `widows`.
- `@page { size; margin }`. Единицы: `px pt mm cm in em %`.

## Зависимости

- `ttf-parser` — cmap, метрики глифов (без зависимостей, no_std).
- `miniz_oxide` — FlateDecode для потоков и шрифтов.
- Всё остальное своё: HTML-токенизатор, CSS, раскладка, PDF-writer.

## Фазы

1. PDF-writer + шрифт: страница с текстом, Type0 + ToUnicode, копирование/поиск в просмотрщике.
2. HTML → DOM, CSS-каскад, computed style.
3. Блочная раскладка + строки (перенос по пробелам, text-align).
4. Пагинация: разрезание блоков по строкам, page-break-*, orphans/widows, `@page`.
5. Таблицы (повтор `thead` на каждой странице), списки, `img` (JPEG как есть, PNG через deflate).
6. Сабсеттинг шрифтов, бенчмарки.

## Вне v1

flex, grid, float, position, JS, внешние ресурсы по сети, двунаправленный текст, лигатуры/шейпинг.
