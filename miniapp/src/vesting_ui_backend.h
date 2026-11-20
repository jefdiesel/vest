#pragma once

#include "rep_vesting_ui_source.h"
#include "logos_ui_plugin_context.h"

#include <QJsonArray>
#include <QString>

/// The Vesting app's MOCK backend. Each slot mirrors one SDK Client method (see
/// ../module/README.md); the real Logos core module can replace this file without touching
/// the QML view. It holds no key and calls no module or network.
class VestingUiBackend : public VestingUiSimpleSource,
                         public LogosUiPluginContext
{
public:
    VestingUiBackend();

    QString list() override;
    QString now() override;
    QString preClaimSummary(qlonglong id, bool isPrivate) override;
    QString create(QString params) override;
    QString createBatch(QString params) override;
    QString claimPublic(qlonglong id) override;
    QString claimPrivate(qlonglong id, QString target) override;
    QString cancel(qlonglong id) override;
    QString makeIrrevocable(qlonglong id) override;
    QString signal(qlonglong id, int index) override;
    QString transfer(qlonglong id, QString newBeneficiary) override;
    QString names() override;
    QString resolveName(QString name) override;

private:
    int indexOf(qlonglong id) const;
    void store(const QJsonObject& schedule, int index);
    QString claimCommon(qlonglong id, bool isPrivate);

    QJsonArray m_schedules;
    QString m_beneficiary;   // the mocked caller / connected wallet account
    QString m_creator;
    qlonglong m_next = 100;
};
