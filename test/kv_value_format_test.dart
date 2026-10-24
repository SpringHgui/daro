import 'package:daro/data/drivers/kv_driver.dart';
import 'package:flutter_test/flutter_test.dart';

// 值面板格式视图的渲染口径(纯函数,不碰 widget):
// 文本 = 原样(裸字节仍是 `HEX:`)、JSON = 解析后缩进、十六进制 = 大写空格分组。
// 守的是两件看不见的事:格式化不能吞掉原文,派生视图必须能原样写回。

void main() {
  group('文本格式', () {
    test('可解码的 UTF-8 给原文,零 note', () {
      final view = kvRenderValue(KvBytes.text('hello'), KvValueFormat.text);
      expect(view.text, 'hello');
      expect(view.note, KvFormatNote.none);
    });

    test('裸字节仍给 HEX: 前缀(编辑框靠它往返)', () {
      final view = kvRenderValue(KvBytes([0x41, 0xff]), KvValueFormat.text);
      expect(view.text, 'HEX:41FF');
      expect(view.note, KvFormatNote.none);
    });

    test('空值给空串,不是 null 也不是 HEX: 前缀', () {
      final view = kvRenderValue(KvBytes(const []), KvValueFormat.text);
      expect(view.text, '');
    });
  });

  group('JSON 格式', () {
    test('对象按两空格缩进重排,键序跟着原文', () {
      final view = kvRenderValue(
          KvBytes.text('{"b":1,"a":[2,{"c":null}]}'), KvValueFormat.json);
      expect(view.text, '{\n  "b": 1,\n  "a": [\n    2,\n    {\n      "c": null\n    }\n  ]\n}');
      expect(view.note, KvFormatNote.none);
    });

    test('裸整数与字符串都是合法 JSON 顶层值', () {
      expect(kvRenderValue(KvBytes.text('42'), KvValueFormat.json).text, '42');
      expect(kvRenderValue(KvBytes.text('"ok"'), KvValueFormat.json).text, '"ok"');
    });

    test('解析失败:给原文 + notJson,不给空白视图', () {
      final view = kvRenderValue(KvBytes.text('{oops'), KvValueFormat.json);
      expect(view.text, '{oops');
      expect(view.note, KvFormatNote.notJson);
    });

    test('二进制值谈不上 JSON:退回 HEX: 并说明原因', () {
      // 0xff 不是合法 UTF-8 起始字节 —— 判据是「解不动」,不是「有控制字符」
      final view = kvRenderValue(KvBytes([0x41, 0xff]), KvValueFormat.json);
      expect(view.text, 'HEX:41FF');
      expect(view.note, KvFormatNote.binary);
    });

    test('超过渲染上限不做半截解析', () {
      final big = '{"a":"' + 'x' * (kKvFormatViewBytes + 1) + '"}';
      final view = kvRenderValue(KvBytes.text(big), KvValueFormat.json);
      expect(view.note, KvFormatNote.tooBig);
      expect(view.text, big, reason: '原文整段仍在,只是没缩进');
    });
  });

  group('十六进制格式', () {
    test('大写、空格分隔,文本值也按字节给', () {
      final view = kvRenderValue(KvBytes.text('Ab '), KvValueFormat.hex);
      expect(view.text, 'HEX:41 62 20');
      expect(view.note, KvFormatNote.none);
    });

    test('零字节仍是可写回的 HEX: 空体', () {
      expect(kvRenderValue(KvBytes(const []), KvValueFormat.hex).text, 'HEX:');
    });

    test('视图能原样写回同一批字节(与编辑框同一口径)', () {
      final bytes = [0x41, 0xff, 0x00, 0x7f];
      final view = kvRenderValue(KvBytes(bytes), KvValueFormat.hex);
      expect(kvBytesFromInput(view.text)?.bytes, bytes);
    });

    test('超出上限只渲染前一段并报 tooBig', () {
      final view = kvRenderValue(
          KvBytes(List.filled(kKvFormatViewBytes + 8, 0xab)), KvValueFormat.hex);
      expect(view.note, KvFormatNote.tooBig);
      final body = view.text.substring(kKvHexPrefix.length);
      expect(body.split(' ').length, kKvFormatViewBytes);
    });

    test('整段不超上限时不报 tooBig(边界:正好等于上限)', () {
      final view = kvRenderValue(
          KvBytes(List.filled(kKvFormatViewBytes, 0xab)), KvValueFormat.hex);
      expect(view.note, KvFormatNote.none);
    });
  });

  test('零字节在三种格式下都有确定输出', () {
    expect(kvRenderValue(KvBytes(const []), KvValueFormat.text).text, '');
    expect(kvRenderValue(KvBytes(const []), KvValueFormat.hex).text, 'HEX:');
    final json = kvRenderValue(KvBytes(const []), KvValueFormat.json);
    expect(json.text, '', reason: '空串不是合法 JSON,但原文仍要在');
    expect(json.note, KvFormatNote.notJson);
  });
}
