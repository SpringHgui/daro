import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:daro/theme/app_theme.dart';
import 'package:daro/theme/sql_highlight.dart';

/// 钉住只读 SQL / DDL 高亮(详情面板 DDL 页、建库改库脚本预览、结构同步脚本)的口径。
///
/// 全部离线:不建驱动、不连库,只喂字符串。
///
/// 最要紧的一条是**文本无损**——高亮器是在原串上做 scope 切分,
/// 一旦某个 scope 把字符吃掉,界面上就是「DDL 少了一截」而不会报错。
void main() {
  const ddl = "CREATE DATABASE `mysql` /*!140100 DEFAULT CHARACTER SET "
      "utf8mb4 COLLATE utf8mb4_0900_ai_ci */ /*!80016 DEFAULT "
      "ENCRYPTION='N' */";

  List<TextSpan> flatten(InlineSpan root) {
    final out = <TextSpan>[];
    void walk(InlineSpan span) {
      if (span is TextSpan) {
        out.add(span);
        for (final child in span.children ?? const <InlineSpan>[]) {
          walk(child);
        }
      }
    }

    walk(root);
    return out;
  }

  String plainText(InlineSpan root) =>
      flatten(root).map((s) => s.text ?? '').join();

  group('文本无损', () {
    test('真实 MySQL 库 DDL 拼接回来与原文逐字相等', () {
      final span = highlightedSqlSpan(ddl, palette: AppTheme.light);
      expect(plainText(span), ddl);
    });

    test('多语句 / 注释 / 空行也不掉字符', () {
      const sql = 'CREATE TABLE `t` (\n'
          '  `id` int NOT NULL AUTO_INCREMENT, -- 主键\n'
          '  `name` varchar(64) DEFAULT NULL,\n'
          '  PRIMARY KEY (`id`)\n'
          ') ENGINE=InnoDB;\n'
          '\n'
          "ALTER TABLE `t` ADD COLUMN `c` int COMMENT 'x';";
      expect(plainText(highlightedSqlSpan(sql, palette: AppTheme.light)), sql);
    });

    test('空串直接给纯文本 span,不进解析器', () {
      final span = highlightedSqlSpan('', palette: AppTheme.light);
      expect(span.toPlainText(), '');
      expect(span.style, isNotNull);
    });

    test('语法上说不通的输入只退回裸文本,不抛', () {
      const junk = "SELECT '未闭合 /* ` ][ \u0000 \u0019 %%% ~~~ ^^^";
      for (final palette in [AppTheme.light, AppTheme.dark]) {
        expect(() => highlightedSqlSpan(junk, palette: palette), returnsNormally);
        expect(plainText(highlightedSqlSpan(junk, palette: palette)), junk,
            reason: '掉了字符就必须整段退回裸文本,界面不能少一截');
      }
    });
  });

  group('确实上了色', () {
    test('明亮主题下 CREATE 走 keyword 色', () {
      final span =
          highlightedSqlSpan('CREATE TABLE t (a INT)', palette: AppTheme.light);
      final keyword = sqlLightTheme['keyword']!.color!;
      // 按「包含」而非「等于」判:语法器可能把 CREATE TABLE 当成一个词组命中。
      expect(
        flatten(span).any((s) =>
            (s.text ?? '').toUpperCase().contains('CREATE') &&
            s.style?.color == keyword),
        isTrue,
        reason: '关键字没上色 = 高亮其实没生效',
      );
    });

    test('暗色主题取的是另一张表', () {
      final span =
          highlightedSqlSpan('CREATE TABLE t (a INT)', palette: AppTheme.dark);
      final darkKeyword = sqlDarkTheme['keyword']!.color!;
      final lightKeyword = sqlLightTheme['keyword']!.color!;
      expect(darkKeyword, isNot(lightKeyword));
      expect(
        flatten(span).any((s) =>
            (s.text ?? '').toUpperCase().contains('CREATE') &&
            s.style?.color == darkKeyword),
        isTrue,
      );
    });

    test('注释是斜体', () {
      final span = highlightedSqlSpan('SELECT 1 -- 备注\n', palette: AppTheme.light);
      expect(
        flatten(span)
            .any((s) => s.style?.fontStyle == FontStyle.italic && s.text != null),
        isTrue,
      );
    });
  });

  group('字号与字体由根 span 兜底', () {
    test('根 span 带统一字体,命中的子 span 不各自塞字号', () {
      final span = highlightedSqlSpan(ddl, palette: AppTheme.light);
      expect(span.style?.fontFamily, 'Consolas');
      expect(span.style?.fontSize, 12);
      expect(span.style?.height, 1.45);
      expect(span.style?.decoration, TextDecoration.none);
      // 没命中任何 scope 的文本直接拿兜底样式(它本来就带字号),要看的是命中的那些。
      final base = sqlCodeStyle(color: AppTheme.light.foreground);
      final themed = flatten(span)
          .where((s) => !identical(s, span) && s.style != base)
          .toList();
      expect(themed, isNotEmpty);
      expect(themed.any((s) => s.style?.fontSize != null), isFalse,
          reason: '子 span 自带字号会盖掉调用方的字号参数');
    });

    test('fontSize / height 参数能覆盖', () {
      final span = highlightedSqlSpan(ddl,
          palette: AppTheme.light, fontSize: 13, height: null);
      expect(span.style?.fontSize, 13);
      expect(span.style?.height, isNull, reason: 'null = 沿用默认行距,各展示位不统一');
      expect(plainText(span), ddl);
    });
  });

  group('memo 缓存', () {
    test('同文本同色板命中同一实例', () {
      final a = highlightedSqlSpan(ddl, palette: AppTheme.light);
      final b = highlightedSqlSpan(ddl, palette: AppTheme.light);
      expect(identical(a, b), isTrue);
    });

    test('换文本 / 换主题都作废', () {
      final a = highlightedSqlSpan(ddl, palette: AppTheme.light);
      expect(identical(a, highlightedSqlSpan('SELECT 1', palette: AppTheme.light)),
          isFalse);
      expect(identical(a, highlightedSqlSpan(ddl, palette: AppTheme.dark)), isFalse);
      expect(
          identical(a,
              highlightedSqlSpan(ddl, palette: AppTheme.light, fontSize: 13)),
          isFalse);
    });
  });

  group('配色表只有一份', () {
    test('明暗两张表的 scope 集合一致', () {
      expect(sqlDarkTheme.keys.toSet(), sqlLightTheme.keys.toSet());
    });

    test('sqlEditorTheme 覆盖 root(编辑器兜底前景随主题)', () {
      final theme =
          sqlEditorTheme(isDark: false, foreground: AppTheme.light.foreground);
      expect(theme.length, sqlLightTheme.length + 1);
      expect(theme['root']!.color, AppTheme.light.foreground);
      expect(theme['keyword']!.color, sqlLightTheme['keyword']!.color);
    });
  });
}
