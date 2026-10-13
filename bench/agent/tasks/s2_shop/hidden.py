from shop.cart import cart_deactivate, cart_new, cart_summary
from shop.common import label_of, regions, tax_rate
from shop.customer import customer_new, customer_sample, customer_tax, customer_total
from shop.invoice import invoice_gross, invoice_new
from shop.lesson import lesson_bucket, lesson_new
from shop.product import product_label, product_line, product_new
from shop.route import route_sample, route_total
from shop.subscription import subscription_new, subscription_renewal_value, subscription_sample, subscription_total, subscription_value


def test_hidden_ca_rate():
    assert tax_rate("CA") == 5
    assert tax_rate("EU") == 19
    assert tax_rate("UK") == 20
    assert tax_rate("APAC") == 0


def test_hidden_ca_used():
    assert customer_tax(customer_new(1, "a", 10, 100, "CA")) == 50
    assert invoice_gross(invoice_new(1, "a", 1, 1000, "CA")) == 1050


def test_hidden_regions():
    assert regions() == ["EU", "US", "UK", "APAC", "CA"]


def test_hidden_label_trim():
    assert label_of("  pen ", 4) == "pen #4"
    assert label_of("box", 12) == "box #12"


def test_hidden_label_used():
    assert product_label(product_new(4, " pen ", 1, 120, "US")) == "pen #4"
    assert product_line(product_new(4, " pen", 1, 1234, "APAC")) == "pen #4: 12.34"


def test_hidden_sub_months():
    assert subscription_new(1, "s", 2, 300, "US", 6).months == 6
    assert subscription_value(subscription_new(1, "s", 2, 300, "US", 6)) == 600


def test_hidden_sub_renewal():
    assert subscription_renewal_value(subscription_new(1, "s", 2, 300, "US", 6)) == 3600
    assert subscription_renewal_value(subscription_new(2, "t", 1, 50, "UK", 1)) == 50


def test_hidden_sub_sample():
    assert [x.months for x in subscription_sample()] == [1, 1, 1]
    assert subscription_total(subscription_sample()) == 1799


def test_hidden_cart_summary():
    assert cart_summary([cart_new(1, "a", 2, 500, "US"), cart_new(2, "b", 1, 1234, "EU"), cart_deactivate(cart_new(3, "c", 1, 999, "US")), cart_new(4, "d", 1, 234, "US"), cart_new(5, "e", 1, 1010, "UK")], "US") == "carts=2 total=13.20"
    assert cart_summary([cart_new(1, "a", 2, 500, "US"), cart_new(2, "b", 1, 1234, "EU"), cart_deactivate(cart_new(3, "c", 1, 999, "US")), cart_new(4, "d", 1, 234, "US"), cart_new(5, "e", 1, 1010, "UK")], "UK") == "carts=1 total=12.12"


def test_hidden_unchanged():
    assert customer_total(customer_sample()) == 886
    assert route_total(route_sample()) == 1308
    assert lesson_bucket(lesson_new(1, "l", 1, 1, "US")) == "small"
