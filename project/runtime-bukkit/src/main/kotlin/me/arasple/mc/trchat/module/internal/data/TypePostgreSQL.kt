package me.arasple.mc.trchat.module.internal.data

import taboolib.expansion.Type
import taboolib.module.database.*

/**
 * PostgreSQL 数据库类型实现
 *
 * @property host PostgreSQL 主机配置
 * @property table 数据表名
 */
class TypePostgreSQL(val host: HostPostgreSQL, val table: String) : Type() {

    val tableVar = Table(table, host) {
        add { id() }
        add("user") {
            type(ColumnTypePostgreSQL.VARCHAR, 36) {
                options(ColumnOptionPostgreSQL.KEY)
            }
        }
        add("key") {
            type(ColumnTypePostgreSQL.VARCHAR, 64) {
                options(ColumnOptionPostgreSQL.KEY)
            }
        }
        add("value") {
            type(ColumnTypePostgreSQL.VARCHAR, 128)
        }
    }

    override fun host(): Host<*> {
        return host
    }

    override fun tableVar(): Table<*, *> {
        return tableVar
    }
}
