#include "vesting_ui_backend.h"

#include <QDateTime>
#include <QJsonDocument>
#include <QJsonObject>

namespace {

const qint64 kDay = 24 * 60 * 60 * 1000LL;
const QString kFee = QStringLiteral("2"); // module-supplied mock fee, as the SDK's fee placeholder

QString answer(const QJsonObject& value)
{
    return QString::fromUtf8(QJsonDocument(value).toJson(QJsonDocument::Compact));
}

QString fail(const QString& why)
{
    return answer({{"ok", false}, {"error", why}});
}

QString normDec(const QString& text)
{
    int i = 0;
    while (i < text.size() - 1 && text.at(i) == QLatin1Char('0'))
        ++i;
    return text.mid(i);
}

unsigned __int128 fromDec(const QString& text)
{
    unsigned __int128 value = 0;
    for (int i = 0; i < text.size(); ++i)
        if (text.at(i).isDigit())
            value = value * 10 + static_cast<unsigned>(text.at(i).digitValue());
    return value;
}

QString toDec(unsigned __int128 value)
{
    if (value == 0)
        return QStringLiteral("0");
    QString out;
    while (value > 0) {
        out.prepend(QString::number(static_cast<int>(value % 10)));
        value /= 10;
    }
    return out;
}

int cmpDec(const QString& a, const QString& b)
{
    const unsigned __int128 x = fromDec(a), y = fromDec(b);
    return x < y ? -1 : (x > y ? 1 : 0);
}

QString subDec(const QString& a, const QString& b) // a >= b
{
    return toDec(fromDec(a) - fromDec(b));
}

QString groupDigits(const QString& digits)
{
    QString out;
    for (int i = 0; i < digits.size(); ++i) {
        if (i > 0 && (digits.size() - i) % 3 == 0)
            out.append(QLatin1Char(','));
        out.append(digits.at(i));
    }
    return out;
}

/// "94138000" with 9 decimals -> "0.094138 LEZ", as the Rust module returns.
QString displayAmount(const QString& amount, int decimals = 9)
{
    QString digits = normDec(amount);
    if (decimals <= 0)
        return groupDigits(digits) + QLatin1String(" LEZ");
    if (digits.size() <= decimals)
        digits = QString(decimals - digits.size() + 1, QLatin1Char('0')) + digits;
    const QString whole = groupDigits(digits.left(digits.size() - decimals));
    QString fraction = digits.right(decimals);
    while (fraction.size() > 1 && fraction.endsWith(QLatin1Char('0')))
        fraction.chop(1);
    return whole + (fraction == QLatin1String("0") ? QString() : QLatin1String(".") + fraction)
           + QLatin1String(" LEZ");
}

/// A base58 account ID with the given prefix (Public/ or Private/).
bool isAccount(const QString& account, const QString& prefix)
{
    if (!account.startsWith(prefix + QLatin1String("/")))
        return false;
    const QString body = account.mid(prefix.size() + 1);
    if (body.size() < 32 || body.size() > 44)
        return false;
    static const QString alphabet = QStringLiteral("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz");
    for (int i = 0; i < body.size(); ++i)
        if (!alphabet.contains(body.at(i)))
            return false;
    return true;
}

/// A base58 account ID: prefix + 33 'a' characters (34 in total).
QString acct(const char* prefix)
{
    return QString::fromLatin1(prefix) + QString(33, QLatin1Char('a'));
}

/// The same exact maths as logic.mjs / vesting-core. __int128 keeps the product exact.
QString vestedAt(const QJsonObject& s, qint64 t)
{
    const QString kind = s.value("kind").toString();
    if (s.value("cancelled").toBool() && s.contains("vestedAtCancel"))
        return normDec(s.value("vestedAtCancel").toString());
    if (kind == QLatin1String("milestone")) {
        const QJsonArray tranches = s.value("tranches").toArray();
        const QJsonArray signalled = s.value("signalled").toArray();
        unsigned __int128 sum = 0;
        for (int i = 0; i < tranches.size() && i < signalled.size(); ++i)
            if (signalled.at(i).toBool())
                sum += fromDec(tranches.at(i).toString());
        return toDec(sum);
    }
    const unsigned __int128 total = fromDec(s.value("total").toString());
    const qint64 start = static_cast<qint64>(s.value("start").toDouble());
    const qint64 end = static_cast<qint64>(s.value("end").toDouble());
    const qint64 cliff = static_cast<qint64>(s.value("cliff").toDouble());
    if (kind == QLatin1String("cliff-linear") && t < cliff)
        return QStringLiteral("0");
    if (t <= start)
        return QStringLiteral("0");
    if (t >= end)
        return toDec(total);
    return toDec(total * static_cast<unsigned __int128>(t - start)
                 / static_cast<unsigned __int128>(end - start));
}

QString claimableAt(const QJsonObject& s, qint64 t)
{
    const QString vested = vestedAt(s, t);
    const QString claimed = normDec(s.value("claimed").toString());
    return cmpDec(vested, claimed) <= 0 ? QStringLiteral("0") : subDec(vested, claimed);
}

/// Mirrors core::next_unlock_after: the first time at which something becomes claimable.
QJsonValue nextUnlockAt(const QJsonObject& s, qint64 t)
{
    if (s.value("cancelled").toBool() || s.value("kind").toString() == QLatin1String("milestone"))
        return QJsonValue();
    const qint64 end = static_cast<qint64>(s.value("end").toDouble());
    if (t >= end)
        return QJsonValue();
    if (s.value("kind").toString() == QLatin1String("cliff-linear")
        && t < static_cast<qint64>(s.value("cliff").toDouble()))
        return QJsonValue(s.value("cliff").toDouble());
    qint64 lo = t + 1, hi = end;
    while (lo < hi) {
        const qint64 mid = lo + (hi - lo) / 2;
        if (cmpDec(claimableAt(s, mid), QStringLiteral("0")) > 0)
            hi = mid;
        else
            lo = mid + 1;
    }
    return QJsonValue(static_cast<double>(lo));
}

QJsonObject schedule(qlonglong id, const QString& kind, const QString& total,
                     const QString& claimed, qint64 start, qint64 cliff, qint64 end,
                     const QJsonArray& tranches, const QJsonArray& signalled,
                     bool cancelable, bool transferable, const QString& beneficiary,
                     const QString& creator)
{
    return {
        {"id", id}, {"kind", kind}, {"token", QStringLiteral("LEZ")},
        {"total", total}, {"claimed", claimed},
        {"start", start}, {"cliff", cliff}, {"end", end},
        {"tranches", tranches}, {"signalled", signalled},
        {"cancelable", cancelable}, {"transferable", transferable}, {"cancelled", false},
        {"beneficiary", beneficiary}, {"creator", creator},
    };
}

QJsonObject paramsObject(const QString& text)
{
    const QJsonDocument doc = QJsonDocument::fromJson(text.trimmed().toUtf8());
    return doc.isObject() ? doc.object() : QJsonObject();
}

} // namespace

VestingUiBackend::VestingUiBackend()
{
    const qint64 now = QDateTime::currentMSecsSinceEpoch();
    m_beneficiary = acct("Public/B");
    m_creator = acct("Public/C");

    const QJsonArray four = {QStringLiteral("100000"), QStringLiteral("100000"),
                             QStringLiteral("100000"), QStringLiteral("100000")};
    const QJsonArray twoSignalled = {true, true, false, false};
    const QJsonArray empty;

    // A cliff 3 days out (nothing claimable yet), a linear schedule part-way through, and a
    // milestone schedule with two of four tranches signalled.
    m_schedules = {
        schedule(1, QStringLiteral("cliff-linear"), QStringLiteral("2000000"), QStringLiteral("0"),
                 now - 180 * kDay, now + 3 * kDay, now + 730 * kDay, empty, empty, true, false,
                 m_beneficiary, m_creator),
        schedule(2, QStringLiteral("linear"), QStringLiteral("1000000"), QStringLiteral("200000"),
                 now - 100 * kDay, 0, now + 264 * kDay, empty, empty, false, true,
                 m_beneficiary, m_creator),
        schedule(3, QStringLiteral("milestone"), QStringLiteral("400000"), QStringLiteral("100000"),
                 now, 0, now + 365 * kDay, four, twoSignalled, true, false,
                 m_beneficiary, m_creator),
    };
}

int VestingUiBackend::indexOf(qlonglong id) const
{
    for (int i = 0; i < m_schedules.size(); ++i)
        if (static_cast<qlonglong>(m_schedules.at(i).toObject().value("id").toDouble()) == id)
            return i;
    return -1;
}

void VestingUiBackend::store(const QJsonObject& s, int index)
{
    m_schedules[index] = s;
}

QString VestingUiBackend::list()
{
    // As the Rust module does: one SDK call, then ready display strings from core maths.
    const qint64 t = QDateTime::currentMSecsSinceEpoch();
    QJsonArray decorated;
    for (const QJsonValue& value : m_schedules) {
        QJsonObject s = value.toObject();
        const QString claim = claimableAt(s, t);
        const QString vested = vestedAt(s, t);
        const QString total = normDec(s.value("total").toString());
        const QString locked = cmpDec(total, vested) > 0 ? subDec(total, vested) : QStringLiteral("0");
        s["claimable"] = claim;
        s["claimable_display"] = displayAmount(claim);
        s["locked_display"] = displayAmount(locked);
        s["total_display"] = displayAmount(total);
        s["claimed_display"] = displayAmount(s.value("claimed").toString());
        decorated.append(s);
    }
    return answer({{"ok", true}, {"schedules", decorated}});
}

QString VestingUiBackend::now()
{
    return answer({{"ok", true}, {"time", QDateTime::currentMSecsSinceEpoch()}});
}

QString VestingUiBackend::preClaimSummary(qlonglong id, bool isPrivate)
{
    const int i = indexOf(id);
    if (i < 0)
        return fail(QStringLiteral("No such schedule."));
    const QJsonObject s = m_schedules.at(i).toObject();
    const qint64 t = QDateTime::currentMSecsSinceEpoch();
    return answer({
        {"ok", true},
        {"summary", {
            {"schedule", static_cast<double>(id)},
            {"claimable", claimableAt(s, t)},
            {"claimable_display", displayAmount(claimableAt(s, t))},
            {"estimated_fee", kFee},
            {"estimated_fee_display", displayAmount(kFee)},
            {"next_unlock", nextUnlockAt(s, t)},
            {"destination_kind", isPrivate ? QStringLiteral("private") : QStringLiteral("public")},
        }},
    });
}

QString VestingUiBackend::claimCommon(qlonglong id, bool isPrivate)
{
    const int i = indexOf(id);
    if (i < 0)
        return fail(QStringLiteral("No such schedule."));
    QJsonObject s = m_schedules.at(i).toObject();
    if (s.value("cancelled").toBool())
        return fail(QStringLiteral("That schedule was cancelled."));
    const QString amount = claimableAt(s, QDateTime::currentMSecsSinceEpoch());
    if (cmpDec(amount, QStringLiteral("0")) <= 0)
        return fail(QStringLiteral("Nothing claimable yet."));
    s["claimed"] = toDec(fromDec(s.value("claimed").toString()) + fromDec(amount));
    store(s, i);
    return answer({{"ok", true}, {"amount", amount}, {"amount_display", displayAmount(amount)},
                   {"claimed_total", s.value("claimed")},
                   {"claimed_total_display", displayAmount(s.value("claimed").toString())},
                   {"to_private", isPrivate},
                   {"tx", QStringLiteral("mock-tx-") + QString::number(id)}});
}

QString VestingUiBackend::claimPublic(qlonglong id)
{
    return claimCommon(id, false);
}

QString VestingUiBackend::claimPrivate(qlonglong id, QString target)
{
    if (!isAccount(target.trimmed(), QStringLiteral("Private")))
        return fail(QStringLiteral("The claim target is not a private (shielded) account."));
    return claimCommon(id, true);
}

QString VestingUiBackend::create(QString params)
{
    QJsonObject s = paramsObject(params);
    const qlonglong id = m_next++;
    s["id"] = static_cast<double>(id);
    s["claimed"] = QStringLiteral("0");
    s["cancelled"] = false;
    s["creator"] = m_creator;
    m_schedules.append(s);
    return answer({{"ok", true}, {"id", static_cast<double>(id)}});
}

QString VestingUiBackend::createBatch(QString params)
{
    const QJsonDocument doc = QJsonDocument::fromJson(params.trimmed().toUtf8());
    if (!doc.isArray())
        return fail(QStringLiteral("The batch is not a list of schedules."));
    QJsonArray ids;
    const QJsonArray rows = doc.array();
    for (int i = 0; i < rows.size(); ++i) {
        QJsonObject s = rows.at(i).toObject();
        const qlonglong id = m_next++;
        s["id"] = static_cast<double>(id);
        s["claimed"] = QStringLiteral("0");
        s["cancelled"] = false;
        s["creator"] = m_creator;
        m_schedules.append(s);
        ids.append(static_cast<double>(id));
    }
    return answer({{"ok", true}, {"ids", ids}});
}

QString VestingUiBackend::cancel(qlonglong id)
{
    const int i = indexOf(id);
    if (i < 0)
        return fail(QStringLiteral("No such schedule."));
    QJsonObject s = m_schedules.at(i).toObject();
    if (!s.value("cancelable").toBool())
        return fail(QStringLiteral("That schedule is not cancelable."));
    if (s.value("cancelled").toBool())
        return fail(QStringLiteral("That schedule was already cancelled."));
    const QString frozen = vestedAt(s, QDateTime::currentMSecsSinceEpoch());
    s["vestedAtCancel"] = frozen;
    s["cancelled"] = true;
    store(s, i);
    const QString refund = subDec(s.value("total").toString(), frozen);
    return answer({{"ok", true}, {"refund", refund}, {"refund_display", displayAmount(refund)}});
}

QString VestingUiBackend::makeIrrevocable(qlonglong id)
{
    const int i = indexOf(id);
    if (i < 0)
        return fail(QStringLiteral("No such schedule."));
    QJsonObject s = m_schedules.at(i).toObject();
    if (!s.value("cancelable").toBool())
        return fail(QStringLiteral("That schedule is already irrevocable."));
    s["cancelable"] = false;
    store(s, i);
    return answer({{"ok", true}});
}

QString VestingUiBackend::signal(qlonglong id, int index)
{
    const int i = indexOf(id);
    if (i < 0)
        return fail(QStringLiteral("No such schedule."));
    QJsonObject s = m_schedules.at(i).toObject();
    if (s.value("kind").toString() != QLatin1String("milestone"))
        return fail(QStringLiteral("That is not a milestone schedule."));
    QJsonArray signalled = s.value("signalled").toArray();
    if (index < 0 || index >= signalled.size())
        return fail(QStringLiteral("No such milestone."));
    if (signalled.at(index).toBool())
        return fail(QStringLiteral("That milestone was already signalled."));
    signalled.replace(index, QJsonValue(true));
    s["signalled"] = signalled;
    store(s, i);
    const QString tranche = s.value("tranches").toArray().at(index).toString();
    return answer({{"ok", true}, {"amount", tranche}, {"amount_display", displayAmount(tranche)}});
}

QString VestingUiBackend::transfer(qlonglong id, QString newBeneficiary)
{
    const int i = indexOf(id);
    if (i < 0)
        return fail(QStringLiteral("No such schedule."));
    QJsonObject s = m_schedules.at(i).toObject();
    if (!s.value("transferable").toBool())
        return fail(QStringLiteral("That position is not transferable."));
    if (!isAccount(newBeneficiary.trimmed(), QStringLiteral("Public"))
        && !isAccount(newBeneficiary.trimmed(), QStringLiteral("Private")))
        return fail(QStringLiteral("That is not a valid account ID."));
    s["beneficiary"] = newBeneficiary.trimmed();
    store(s, i);
    return answer({{"ok", true}});
}

// ── Name seam (mock until the SDK's names module lands) ─────────────────────────────

namespace {
QJsonObject mockNames(const QString& beneficiary)
{
    const QJsonArray destinations = {
        QJsonObject{{"kind", QStringLiteral("public")}, {"account", acct("Public/D")}},
        QJsonObject{{"kind", QStringLiteral("private")}, {"account", acct("Private/P")}},
    };
    return {
        {"alice", beneficiary},
        {"bob", destinations.at(0).toObject().value("account")},
        {"carol", destinations.at(1).toObject().value("account")},
    };
}
} // namespace

QString VestingUiBackend::names()
{
    return answer({{"ok", true}, {"names", mockNames(m_beneficiary)}});
}

QString VestingUiBackend::resolveName(QString name)
{
    const QString key = name.trim().toLower();
    const QJsonObject map = mockNames(m_beneficiary);
    if (key.isEmpty() || !map.contains(key))
        return fail(QStringLiteral("No LNS name '%1' is known to this mock.").arg(name));
    return answer({{"ok", true}, {"name", key}, {"account", map.value(key)}});
}
