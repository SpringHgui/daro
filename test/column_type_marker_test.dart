import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/widgets/table_data_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('columnTypeMarker 按类型族分派表头前导标记', () {
    test('数值族 → #', () {
      for (final t in [
        'bigint(20)',
        'int(11)',
        'decimal(10,2)',
        'double precision',
        'tinyint(1)',
        'bit(1)',
        'bit(8)',
        'serial',
      ]) {
        expect(columnTypeMarker(t).glyph, '#', reason: t);
        expect(columnTypeMarker(t).icon, isNull, reason: t);
      }
    });

    test('文本族 → abc', () {
      for (final t in [
        'varchar(64)',
        'character varying(20)',
        'nvarchar(max)',
        'text',
        'uuid',
      ]) {
        expect(columnTypeMarker(t).glyph, 'abc', reason: t);
        expect(columnTypeMarker(t).icon, isNull, reason: t);
      }
    });

    test('日期时间族 → 时钟图标(各驱动写法)', () {
      for (final t in [
        'datetime',
        'datetime(6)',
        'DATETIME',
        'smalldatetime',
        'datetime2',
        'date',
        'time(6)',
        'timestamp',
        'timestamp with time zone',
        'timestamptz',
        'year(4)',
        'Date/Time',
      ]) {
        final m = columnTypeMarker(t);
        expect(m.icon, Icons.schedule, reason: t);
        expect(m.glyph, isNull, reason: t);
      }
    });

    test('interval 归日期时间族,不被数值族词根 int 抢走', () {
      expect(columnTypeMarker('interval').icon, Icons.schedule);
      expect(columnTypeMarker('interval').glyph, isNull);
    });

    test('其余类型与空值都不画标记', () {
      for (final t in ['point', 'blob', 'json', 'geometry', '', null]) {
        final m = columnTypeMarker(t);
        expect(m.glyph, isNull, reason: '$t');
        expect(m.icon, isNull, reason: '$t');
      }
    });
  });

  group('columnDateTimeMode 决定行内编辑器挂哪种选择器', () {
    test('日期时间族 → dateTime', () {
      for (final t in [
        'datetime',
        'datetime(6)',
        'DATETIME',
        'smalldatetime',
        'datetime2',
        'datetimeoffset',
        'timestamp',
        'timestamp with time zone',
        'timestamptz',
        'Date/Time',
      ]) {
        expect(columnDateTimeMode(t), DateTimePickerMode.dateTime, reason: t);
      }
    });

    test('裸 date → date，裸 time → time', () {
      expect(columnDateTimeMode('date'), DateTimePickerMode.date);
      expect(columnDateTimeMode('DATE'), DateTimePickerMode.date);
      expect(columnDateTimeMode('time'), DateTimePickerMode.time);
      expect(columnDateTimeMode('time(6)'), DateTimePickerMode.time);
      expect(columnDateTimeMode('time with time zone'), DateTimePickerMode.time);
      expect(columnDateTimeMode('timetz'), DateTimePickerMode.time);
    });

    test('year / interval / 旧 pg 时刻类型不给选择器', () {
      for (final t in ['year(4)', 'interval', 'abstime', 'reltime']) {
        expect(columnDateTimeMode(t), isNull, reason: t);
      }
    });

    test('非日期时间族一律 null', () {
      for (final t in ['bigint(20)', 'varchar(64)', 'json', '', null]) {
        expect(columnDateTimeMode(t), isNull, reason: '$t');
      }
    });
  });
}
