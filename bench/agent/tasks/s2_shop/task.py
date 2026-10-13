"""s2_shop: four targeted changes on the s1_shop codebase (a different change set from s1)."""
import os, re, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "s1_shop"))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "large"))
import gen  # noqa: E402  s1_shop/gen.py
from lib import C, Eq, F, L, P  # noqa: E402

CODEBASE = "shop"
TITLE = "Shop, second task: four targeted changes in a large codebase"
EXAMPLE = "label_of is labelOf"
STEPS = [
    "Add the region CA with a tax rate of 5%; regions() lists it last.",
    "Bug: label_of keeps stray spaces from names (\" pen \" with id 4 gives \" pen  #4\"). It must trim the name: \"pen #4\".",
    "Subscriptions get a length in months: subscription_new gets a new last parameter months (an Int), stored in a new field months of Subscription. Every existing caller passes 1. Add subscription_renewal_value(x): subscription_value(x) times x.months.",
    "Add cart_summary(xs, r): for the active carts in region r, \"carts=N total=T\", where N is how many there are and T is money of their cart_total (for example \"carts=2 total=13.20\").",
    "Add a test for each change. All existing tests must still pass.",
]
IFACE = {
    "sspur": "fn subscription_new(id: Int, name: Str, qty: Int, price: Int, region: Str, months: Int) -> Subscription (Subscription gets a field months: Int); fn subscription_renewal_value(x: Subscription) -> Int; fn cart_summary(xs: List[Cart], r: Str) -> Str; tax_rate, label_of, regions and the other existing functions keep their signatures",
    "python": "shop.subscription.subscription_new(id, name, qty, price, region, months) -> Subscription (a new field months); shop.subscription.subscription_renewal_value(x) -> int; shop.cart.cart_summary(xs, r) -> str; tax_rate, label_of, regions and the other existing functions keep their modules and signatures",
    "ts": "shop/subscription.ts: export function subscriptionNew(id: number, name: string, qty: number, price: number, region: string, months: number): Subscription (Subscription gets readonly months: number); export function subscriptionRenewalValue(x: Subscription): number; shop/cart.ts: export function cartSummary(xs: readonly Cart[], r: string): string; taxRate, labelOf, regions and the other existing functions keep their modules and signatures",
    "go": "func subscriptionNew(id int, name string, qty int, price int, region string, months int) Subscription (Subscription gets a field months int); func subscriptionRenewalValue(x Subscription) int; func cartSummary(xs []Cart, r string) string, all in package shop; taxRate, labelOf, regions and the other existing functions keep their signatures",
}

SUB_TOTAL = gen.total(gen.params(gen.DOMAINS.index("subscription"))[3], False)


def sub(*a):
    return C("subscription_new", *a)


def cart(*a):
    return C("cart_new", *a)


CARTS = L("Cart", [cart(1, "a", 2, 500, "US"), cart(2, "b", 1, 1234, "EU"), C("cart_deactivate", cart(3, "c", 1, 999, "US")),
                   cart(4, "d", 1, 234, "US"), cart(5, "e", 1, 1010, "UK")])

HIDDEN = [
    ("hidden_ca_rate", [Eq(C("tax_rate", "CA"), 5), Eq(C("tax_rate", "EU"), 19), Eq(C("tax_rate", "UK"), 20), Eq(C("tax_rate", "APAC"), 0)]),
    ("hidden_ca_used", [Eq(C("customer_tax", C("customer_new", 1, "a", 10, 100, "CA")), 50),
                        Eq(C("invoice_gross", C("invoice_new", 1, "a", 1, 1000, "CA")), 1050)]),
    ("hidden_regions", [Eq(C("regions"), ["EU", "US", "UK", "APAC", "CA"])]),
    ("hidden_label_trim", [Eq(C("label_of", "  pen ", 4), "pen #4"), Eq(C("label_of", "box", 12), "box #12")]),
    ("hidden_label_used", [Eq(C("product_label", C("product_new", 4, " pen ", 1, 120, "US")), "pen #4"),
                           Eq(C("product_line", C("product_new", 4, " pen", 1, 1234, "APAC")), "pen #4: 12.34")]),
    ("hidden_sub_months", [Eq(F(sub(1, "s", 2, 300, "US", 6), "months"), 6),
                           Eq(C("subscription_value", sub(1, "s", 2, 300, "US", 6)), 600)]),
    ("hidden_sub_renewal", [Eq(C("subscription_renewal_value", sub(1, "s", 2, 300, "US", 6)), 3600),
                            Eq(C("subscription_renewal_value", sub(2, "t", 1, 50, "UK", 1)), 50)]),
    ("hidden_sub_sample", [Eq(P(C("subscription_sample"), "Subscription", "months", "Int"), [1, 1, 1]),
                           Eq(C("subscription_total", C("subscription_sample")), SUB_TOTAL)]),
    ("hidden_cart_summary", [Eq(C("cart_summary", CARTS, "US"), "carts=2 total=13.20"),
                             Eq(C("cart_summary", CARTS, "UK"), "carts=1 total=12.12")]),
    ("hidden_unchanged", [Eq(C("customer_total", C("customer_sample")), 886), Eq(C("route_total", C("route_sample")), 1308),
                          Eq(C("lesson_bucket", C("lesson_new", 1, "l", 1, 1, "US")), "small")]),
]

NEW_ARGS = re.compile(r'(subscription_new|subscriptionNew)\((\d+, "[^"]*", \d+, \d+, "[A-Z]+")\)')


def patches():
    return {
        "sspur": [
            ("start.ssp", '  | "UK" => 20\n', '  | "UK" => 20\n  | "CA" => 5\n'),
            ("start.ssp", '= ["EU", "US", "UK", "APAC"]', '= ["EU", "US", "UK", "APAC", "CA"]'),
            ("start.ssp", '= "{name} #{id}"', '= "{name.trim} #{id}"'),
            ("start.ssp", "type Subscription = {id: Int, name: Str, qty: Int, price: Int, region: Str, active: Bool}",
             "type Subscription = {id: Int, name: Str, qty: Int, price: Int, region: Str, active: Bool, months: Int}"),
            ("start.ssp", "fn subscription_new(id: Int, name: Str, qty: Int, price: Int, region: Str) -> Subscription\n= Subscription{id, name, qty, price, region, active: true}",
             "fn subscription_new(id: Int, name: Str, qty: Int, price: Int, region: Str, months: Int) -> Subscription\n= Subscription{id, name, qty, price, region, active: true, months}"),
            ("start.ssp", NEW_ARGS, r"\1(\2, 1)", 5),
            ("start.ssp", None, "fn subscription_renewal_value(x: Subscription) -> Int\n= subscription_value(x) * x.months\n\n"
             "fn cart_summary(xs: List[Cart], r: Str) -> Str\n= do\n  ys = cart_in_region(xs, r).filter(_.active)\n  \"carts={ys.len} total={money(cart_total(ys))}\"\n\n"
             "test s2_ca = tax_rate(\"CA\") == 5 and regions().last == some(\"CA\")\n\n"
             "test s2_label = label_of(\" pen \", 4) == \"pen #4\"\n\n"
             "test s2_renewal = subscription_renewal_value(subscription_new(1, \"s\", 2, 300, \"US\", 6)) == 3600\n\n"
             "test s2_cart_summary = cart_summary([cart_new(1, \"a\", 2, 500, \"US\"), cart_new(4, \"d\", 1, 234, \"US\")], \"US\") == \"carts=2 total=13.20\""),
        ],
        "python": [
            ("shop/common.py", '    if region == "UK":\n        return 20\n', '    if region == "UK":\n        return 20\n    if region == "CA":\n        return 5\n'),
            ("shop/common.py", 'return ["EU", "US", "UK", "APAC"]', 'return ["EU", "US", "UK", "APAC", "CA"]'),
            ("shop/common.py", 'return f"{name} #{id}"', 'return f"{name.strip()} #{id}"'),
            ("shop/subscription.py", "    active: bool\n", "    active: bool\n    months: int\n"),
            ("shop/subscription.py", "def subscription_new(id: int, name: str, qty: int, price: int, region: str) -> Subscription:\n    return Subscription(id, name, qty, price, region, True)",
             "def subscription_new(id: int, name: str, qty: int, price: int, region: str, months: int) -> Subscription:\n    return Subscription(id, name, qty, price, region, True, months)"),
            ("shop/subscription.py", NEW_ARGS, r"\1(\2, 1)", 3),
            ("tests/test_subscription.py", NEW_ARGS, r"\1(\2, 1)", 2),
            ("shop/subscription.py", None, "\ndef subscription_renewal_value(x: Subscription) -> int:\n    return subscription_value(x) * x.months"),
            ("shop/cart.py", None, "\ndef cart_summary(xs: List[Cart], r: str) -> str:\n    ys = [x for x in cart_in_region(xs, r) if x.active]\n    return f\"carts={len(ys)} total={money(cart_total(ys))}\""),
        ],
        "ts": [
            ("shop/common.ts", '    case "UK":\n      return 20;\n', '    case "UK":\n      return 20;\n    case "CA":\n      return 5;\n'),
            ("shop/common.ts", 'return ["EU", "US", "UK", "APAC"];', 'return ["EU", "US", "UK", "APAC", "CA"];'),
            ("shop/common.ts", "return `${name} #${id}`;", "return `${name.trim()} #${id}`;"),
            ("shop/subscription.ts", "  readonly active: boolean;\n", "  readonly active: boolean;\n  readonly months: number;\n"),
            ("shop/subscription.ts", "export function subscriptionNew(id: number, name: string, qty: number, price: number, region: string): Subscription {\n  return { id, name, qty, price, region, active: true };",
             "export function subscriptionNew(id: number, name: string, qty: number, price: number, region: string, months: number): Subscription {\n  return { id, name, qty, price, region, active: true, months };"),
            ("shop/subscription.ts", NEW_ARGS, r"\1(\2, 1)", 3),
            ("tests/subscription.test.ts", NEW_ARGS, r"\1(\2, 1)", 2),
            ("shop/subscription.ts", None, "export function subscriptionRenewalValue(x: Subscription): number {\n  return subscriptionValue(x) * x.months;\n}"),
            ("shop/cart.ts", None, "export function cartSummary(xs: readonly Cart[], r: string): string {\n  const ys = cartInRegion(xs, r).filter((x) => x.active);\n  return `carts=${ys.length} total=${money(cartTotal(ys))}`;\n}"),
        ],
        "go": [
            ("shop/common.go", '\tcase "UK":\n\t\treturn 20\n', '\tcase "UK":\n\t\treturn 20\n\tcase "CA":\n\t\treturn 5\n'),
            ("shop/common.go", '[]string{"EU", "US", "UK", "APAC"}', '[]string{"EU", "US", "UK", "APAC", "CA"}'),
            ("shop/common.go", 'fmt.Sprintf("%s #%d", name, id)', 'fmt.Sprintf("%s #%d", strings.TrimSpace(name), id)'),
            ("shop/subscription.go", "\tactive bool\n}", "\tactive bool\n\tmonths int\n}"),
            ("shop/subscription.go", "func subscriptionNew(id int, name string, qty int, price int, region string) Subscription {\n\treturn Subscription{id, name, qty, price, region, true}",
             "func subscriptionNew(id int, name string, qty int, price int, region string, months int) Subscription {\n\treturn Subscription{id, name, qty, price, region, true, months}"),
            ("shop/subscription.go", NEW_ARGS, r"\1(\2, 1)", 3),
            ("shop/subscription_test.go", NEW_ARGS, r"\1(\2, 1)", 2),
            ("shop/subscription.go", None, "func subscriptionRenewalValue(x Subscription) int {\n\treturn subscriptionValue(x) * x.months\n}"),
            ("shop/cart.go", 'import "sort"', 'import (\n\t"fmt"\n\t"sort"\n)'),
            ("shop/cart.go", None, "func cartSummary(xs []Cart, r string) string {\n\tys := []Cart{}\n\tfor _, x := range cartInRegion(xs, r) {\n\t\tif x.active {\n\t\t\tys = append(ys, x)\n\t\t}\n\t}\n\treturn fmt.Sprintf(\"carts=%d total=%s\", len(ys), money(cartTotal(ys)))\n}"),
        ],
    }
