// SPDX-License-Identifier: Apache-2.0
#ifndef JADE_ENGINE_JSON_MINI_HPP_
#define JADE_ENGINE_JSON_MINI_HPP_

#include <charconv>
#include <cstdint>
#include <cstdlib>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

namespace jade::json {

struct Value;
using Array = std::vector<Value>;
using Object = std::map<std::string, Value, std::less<>>;

struct Value {
  using Storage = std::variant<std::nullptr_t, bool, int64_t, double, std::string, Array, Object>;
  Storage data = nullptr;

  bool is_null() const { return std::holds_alternative<std::nullptr_t>(data); }
  bool is_bool() const { return std::holds_alternative<bool>(data); }
  bool is_int() const { return std::holds_alternative<int64_t>(data); }
  bool is_number() const {
    return std::holds_alternative<int64_t>(data) || std::holds_alternative<double>(data);
  }
  bool is_string() const { return std::holds_alternative<std::string>(data); }
  bool is_array() const { return std::holds_alternative<Array>(data); }
  bool is_object() const { return std::holds_alternative<Object>(data); }

  bool as_bool(bool fallback = false) const {
    if (const auto* b = std::get_if<bool>(&data)) return *b;
    return fallback;
  }

  int64_t as_int(int64_t fallback = 0) const {
    if (const auto* i = std::get_if<int64_t>(&data)) return *i;
    if (const auto* d = std::get_if<double>(&data)) return static_cast<int64_t>(*d);
    return fallback;
  }

  std::string_view as_string_view(std::string_view fallback = {}) const {
    if (const auto* s = std::get_if<std::string>(&data)) return *s;
    return fallback;
  }

  const Array* as_array() const { return std::get_if<Array>(&data); }
  const Object* as_object() const { return std::get_if<Object>(&data); }

  const Value* find(std::string_view key) const {
    const auto* obj = as_object();
    if (!obj) return nullptr;
    auto it = obj->find(key);
    if (it == obj->end()) return nullptr;
    return &it->second;
  }
};

class Parser {
 public:
  explicit Parser(std::string_view src) : src_(src) {}

  bool parse(Value& out, std::string& err) {
    skip_ws();
    if (!parse_value(out, err, 0)) {
      return false;
    }
    skip_ws();
    if (pos_ != src_.size()) {
      err = "unexpected trailing characters in JSON input";
      return false;
    }
    return true;
  }

 private:
  static constexpr int kMaxDepth = 32;

  void skip_ws() {
    while (pos_ < src_.size()) {
      char c = src_[pos_];
      if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
        ++pos_;
      } else {
        break;
      }
    }
  }

  bool parse_value(Value& out, std::string& err, int depth) {
    if (depth > kMaxDepth) {
      err = "JSON nesting depth exceeded";
      return false;
    }
    skip_ws();
    if (pos_ >= src_.size()) {
      err = "unexpected end of JSON input";
      return false;
    }
    char c = src_[pos_];
    if (c == '{') return parse_object(out, err, depth + 1);
    if (c == '[') return parse_array(out, err, depth + 1);
    if (c == '"') {
      std::string s;
      if (!parse_string(s, err)) return false;
      out.data = std::move(s);
      return true;
    }
    if (c == 't') return consume_literal("true", true, out, err);
    if (c == 'f') return consume_literal("false", false, out, err);
    if (c == 'n') {
      if (src_.substr(pos_, 4) == "null") {
        pos_ += 4;
        out.data = nullptr;
        return true;
      }
      err = "invalid literal in JSON input";
      return false;
    }
    if (c == '-' || (c >= '0' && c <= '9')) {
      return parse_number(out, err);
    }
    err = "unexpected token in JSON input";
    return false;
  }

  bool consume_literal(std::string_view lit, bool val, Value& out, std::string& err) {
    if (src_.substr(pos_, lit.size()) == lit) {
      pos_ += lit.size();
      out.data = val;
      return true;
    }
    err = "invalid boolean literal in JSON input";
    return false;
  }

  bool parse_string(std::string& out, std::string& err) {
    if (pos_ >= src_.size() || src_[pos_] != '"') {
      err = "expected string opening quote";
      return false;
    }
    ++pos_;
    out.clear();
    while (pos_ < src_.size()) {
      char c = src_[pos_++];
      if (c == '"') {
        return true;
      }
      if (static_cast<unsigned char>(c) < 0x20u) {
        err = "unescaped control character in JSON string";
        return false;
      }
      if (c != '\\') {
        out.push_back(c);
        continue;
      }
      if (pos_ >= src_.size()) {
        err = "unterminated escape in JSON string";
        return false;
      }
      char esc = src_[pos_++];
      switch (esc) {
        case '"': out.push_back('"'); break;
        case '\\': out.push_back('\\'); break;
        case '/': out.push_back('/'); break;
        case 'b': out.push_back('\b'); break;
        case 'f': out.push_back('\f'); break;
        case 'n': out.push_back('\n'); break;
        case 'r': out.push_back('\r'); break;
        case 't': out.push_back('\t'); break;
        case 'u': {
          if (pos_ + 4 > src_.size()) {
            err = "incomplete unicode escape in JSON string";
            return false;
          }
          uint32_t cp = 0;
          for (int i = 0; i < 4; ++i) {
            char h = src_[pos_++];
            cp <<= 4;
            if (h >= '0' && h <= '9') cp |= static_cast<uint32_t>(h - '0');
            else if (h >= 'a' && h <= 'f') cp |= static_cast<uint32_t>(10 + h - 'a');
            else if (h >= 'A' && h <= 'F') cp |= static_cast<uint32_t>(10 + h - 'A');
            else {
              err = "invalid hex digit in unicode escape";
              return false;
            }
          }
          append_utf8(out, cp);
          break;
        }
        default:
          err = "invalid escape character in JSON string";
          return false;
      }
    }
    err = "unterminated JSON string";
    return false;
  }

  static void append_utf8(std::string& out, uint32_t cp) {
    if (cp <= 0x7fu) {
      out.push_back(static_cast<char>(cp));
    } else if (cp <= 0x7ffu) {
      out.push_back(static_cast<char>(0xc0u | ((cp >> 6) & 0x1fu)));
      out.push_back(static_cast<char>(0x80u | (cp & 0x3fu)));
    } else {
      out.push_back(static_cast<char>(0xe0u | ((cp >> 12) & 0x0fu)));
      out.push_back(static_cast<char>(0x80u | ((cp >> 6) & 0x3fu)));
      out.push_back(static_cast<char>(0x80u | (cp & 0x3fu)));
    }
  }

  bool parse_number(Value& out, std::string& err) {
    size_t start = pos_;
    if (src_[pos_] == '-') ++pos_;
    if (pos_ >= src_.size() || src_[pos_] < '0' || src_[pos_] > '9') {
      err = "invalid number in JSON input";
      return false;
    }
    if (src_[pos_] == '0') {
      ++pos_;
    } else {
      while (pos_ < src_.size() && src_[pos_] >= '0' && src_[pos_] <= '9') {
        ++pos_;
      }
    }
    bool is_float = false;
    if (pos_ < src_.size() && src_[pos_] == '.') {
      is_float = true;
      ++pos_;
      if (pos_ >= src_.size() || src_[pos_] < '0' || src_[pos_] > '9') {
        err = "invalid fractional number in JSON input";
        return false;
      }
      while (pos_ < src_.size() && src_[pos_] >= '0' && src_[pos_] <= '9') {
        ++pos_;
      }
    }
    if (pos_ < src_.size() && (src_[pos_] == 'e' || src_[pos_] == 'E')) {
      is_float = true;
      ++pos_;
      if (pos_ < src_.size() && (src_[pos_] == '+' || src_[pos_] == '-')) {
        ++pos_;
      }
      if (pos_ >= src_.size() || src_[pos_] < '0' || src_[pos_] > '9') {
        err = "invalid exponent in JSON number";
        return false;
      }
      while (pos_ < src_.size() && src_[pos_] >= '0' && src_[pos_] <= '9') {
        ++pos_;
      }
    }

    std::string_view token = src_.substr(start, pos_ - start);
    if (!is_float) {
      int64_t val = 0;
      auto [ptr, ec] = std::from_chars(token.data(), token.data() + token.size(), val);
      if (ec == std::errc{} && ptr == token.data() + token.size()) {
        out.data = val;
        return true;
      }
    }
    std::string tmp(token);
    char* end_ptr = nullptr;
    double dval = std::strtod(tmp.c_str(), &end_ptr);
    if (end_ptr != tmp.c_str() + tmp.size()) {
      err = "failed to parse JSON number";
      return false;
    }
    out.data = dval;
    return true;
  }

  bool parse_array(Value& out, std::string& err, int depth) {
    ++pos_;  // '['
    Array arr;
    skip_ws();
    if (pos_ < src_.size() && src_[pos_] == ']') {
      ++pos_;
      out.data = std::move(arr);
      return true;
    }
    while (true) {
      Value elem;
      if (!parse_value(elem, err, depth)) return false;
      arr.push_back(std::move(elem));
      skip_ws();
      if (pos_ >= src_.size()) {
        err = "unterminated JSON array";
        return false;
      }
      if (src_[pos_] == ']') {
        ++pos_;
        out.data = std::move(arr);
        return true;
      }
      if (src_[pos_] != ',') {
        err = "expected ',' or ']' in JSON array";
        return false;
      }
      ++pos_;
    }
  }

  bool parse_object(Value& out, std::string& err, int depth) {
    ++pos_;  // '{'
    Object obj;
    skip_ws();
    if (pos_ < src_.size() && src_[pos_] == '}') {
      ++pos_;
      out.data = std::move(obj);
      return true;
    }
    while (true) {
      skip_ws();
      std::string key;
      if (!parse_string(key, err)) return false;
      skip_ws();
      if (pos_ >= src_.size() || src_[pos_] != ':') {
        err = "expected ':' after JSON object key";
        return false;
      }
      ++pos_;
      Value val;
      if (!parse_value(val, err, depth)) return false;
      obj[std::move(key)] = std::move(val);
      skip_ws();
      if (pos_ >= src_.size()) {
        err = "unterminated JSON object";
        return false;
      }
      if (src_[pos_] == '}') {
        ++pos_;
        out.data = std::move(obj);
        return true;
      }
      if (src_[pos_] != ',') {
        err = "expected ',' or '}' in JSON object";
        return false;
      }
      ++pos_;
    }
  }

  std::string_view src_;
  size_t pos_ = 0;
};

inline bool parse(std::string_view src, Value& out, std::string& err) {
  Parser p(src);
  return p.parse(out, err);
}

inline void append_escaped_string(std::string& out, std::string_view sv) {
  out.push_back('"');
  static constexpr char kHex[] = "0123456789abcdef";
  for (unsigned char c : sv) {
    switch (c) {
      case '"': out += "\\\""; break;
      case '\\': out += "\\\\"; break;
      case '\b': out += "\\b"; break;
      case '\f': out += "\\f"; break;
      case '\n': out += "\\n"; break;
      case '\r': out += "\\r"; break;
      case '\t': out += "\\t"; break;
      default:
        if (c < 0x20u) {
          out += "\\u00";
          out.push_back(kHex[(c >> 4) & 0x0fu]);
          out.push_back(kHex[c & 0x0fu]);
        } else {
          out.push_back(static_cast<char>(c));
        }
        break;
    }
  }
  out.push_back('"');
}

}  // namespace jade::json

#endif  // JADE_ENGINE_JSON_MINI_HPP_
