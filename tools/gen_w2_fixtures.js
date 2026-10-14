// W2 fixtures produced by the reference's own code paths:
//  * email_cases: validator.js isEmail (as class-validator's @IsEmail() calls it)
//  * line_amounts: invoice/bill line amount = Number((q * p).toFixed(2)) for 2-dp inputs,
//    to measure where JavaScript binary rounding differs from exact half-up (deviation D4).
// Usage: node tools/gen_w2_fixtures.js <reference backend dir> <output json>
const path = require('path');
const fs = require('fs');
const ref = process.argv[2];
const isEmail = require(path.join(ref, 'node_modules', 'validator', 'lib', 'isEmail')).default;

const emails = [
  'a@b.co', 'billing@supplier.lk', 'first.last@example.com', 'first+tag@example.com', 'x@y', 'bad@', '@b.com',
  'plainaddress', 'a b@c.com', 'a@b c.com', 'a@b..com', 'a..b@c.com', '.a@c.com', 'a.@c.com', 'a@-b.com', 'a@b-.com',
  'a@b.c', 'a@b.c0m', 'a@b.123', 'a@123.com', 'a@b_c.com', 'user@localhost', 'user@[192.168.0.1]', 'user@192.168.0.1',
  '"quoted"@example.com', '"a b"@example.com', 'üñî@example.com', 'user@exämple.com', 'user@xn--exmple-cua.com',
  'Name <a@b.com>', 'a@b.com ', ' a@b.com', 'A@B.COM', 'a@b.co.uk', 'a_b@c.com', "o'reilly@example.com",
  'a!#$%&*+/=?^`{|}~-@example.com', 'a@b.museum', 'a@b.verylongtldxxxxxxxxxxxxxxxxxxxxxx', 'a@' + 'b'.repeat(64) + '.com',
  'x'.repeat(64) + '@example.com', 'x'.repeat(65) + '@example.com', 'a@' + ('b'.repeat(63) + '.').repeat(4) + 'com',
  'a@b.com.', 'a@.b.com', 'a@b..c', 'a"b@c.com', 'a\\b@c.com', 'a@b.c-d', 'a@b.-cd', 'a@bb.c_d', 'test@test.co1',
  '', ' ', 'a@b.com\n', 'mailto:a@b.com',
];
const email_cases = emails.map((e) => [e, isEmail(e)]);

// Deterministic pseudo-random 2-dp quantities and prices, plus known-awkward pairs.
let seed = 12345;
const rnd = (n) => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed % n; };
const pairs = [[0.29, 0.5], [1.005, 1], [2.5, 1000.25], [0.07, 0.5], [1.15, 1.5], [0.35, 0.3], [3, 33.33], [0.01, 0.5]];
for (let i = 0; i < 20000; i++) pairs.push([(1 + rnd(100000)) / 100, rnd(10000000) / 100]);
const line_amounts = pairs.map(([q, p]) => [q, p, Number((q * p).toFixed(2))]);

fs.writeFileSync(process.argv[3], JSON.stringify({ email_cases, line_amounts }));
console.log(`${email_cases.length} emails, ${line_amounts.length} line amounts`);
